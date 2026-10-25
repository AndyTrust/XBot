//! Fase 2.E S-B: computer collegati con un lavoratore finto in-process (tokio-tungstenite).
//! Collega, battito, lavoro, eventi, chiusura, ripresa dopo il taglio del socket, lease
//! scaduto, generazione vecchia rifiutata, attesa del Mac, ripiego Auto → VPS, richieste,
//! squadra, rimozione.

mod comune;

use comune::*;
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use xbot_server::canale_macchina::Tempi;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Banco {
    p: Prova,
    cookie: String,
    indirizzo: String,
}

async fn banco() -> Banco {
    let p = prova(0).await;
    let cookie = p.entra().await;
    let ascolto = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let indirizzo = ascolto.local_addr().unwrap().to_string();
    let app = p.app.clone();
    tokio::spawn(async move { axum::serve(ascolto, app).await.unwrap() });
    Banco {
        p,
        cookie,
        indirizzo,
    }
}

fn provider_mac() -> Value {
    json!([
        { "id": "cursor_agent", "nome": "Cursor", "programma": "/usr/local/bin/agent",
          "versione": "2026.10.14",
          "modelli": [ { "id": "grok-4.7", "etichetta": "Grok 4.7 (Cursor)", "livelli": [] } ] },
        { "id": "codex", "nome": "Codex / ChatGPT", "programma": "codex",
          "modelli": [ { "id": "gpt-5.2", "etichetta": "GPT 5.2" } ] }
    ])
}

impl Banco {
    async fn chiama(&self, metodo: &str, uri: &str, corpo: Option<Value>) -> (StatusCode, Value) {
        let (s, v, _) = self.p.chiama(metodo, uri, corpo, Some(&self.cookie)).await;
        (s, v)
    }

    /// Biglietto e `collega`: (macchina_id, credenziale).
    async fn collega(&self) -> (String, String) {
        let (s, b) = self
            .chiama("POST", "/api/v1/macchine/biglietto", None)
            .await;
        assert_eq!(s, StatusCode::OK, "{b}");
        let corpo = json!({
            "biglietto": b["biglietto"], "nome": "mac", "genere": "mac", "os": "macos",
            "arch": "aarch64", "versione_lavoratore": "0.1.0", "chiave_pubblica": "SPKI",
            "posti": 2, "capacita": ["turni", "chrome", "file"], "provider": provider_mac(),
        });
        let (s, v, _) = self
            .p
            .chiama(
                "POST",
                "/api/v1/macchine/collega",
                Some(corpo.clone()),
                None,
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        // Il biglietto vale una volta.
        let (s2, _, _) = self
            .p
            .chiama("POST", "/api/v1/macchine/collega", Some(corpo), None)
            .await;
        assert_eq!(s2, StatusCode::UNAUTHORIZED);
        (
            v["macchina_id"].as_str().unwrap().to_string(),
            v["credenziale"].as_str().unwrap().to_string(),
        )
    }

    async fn apri(&self, mid: &str, cred: &str) -> Result<Socket, String> {
        let url = format!("ws://{}/api/v1/macchine/{mid}/canale", self.indirizzo);
        let mut req = url.into_client_request().unwrap();
        req.headers_mut()
            .insert("authorization", format!("Bearer {cred}").parse().unwrap());
        tokio_tungstenite::connect_async(req)
            .await
            .map(|(s, _)| s)
            .map_err(|e| e.to_string())
    }

    /// Assistente con provider e macchina predefiniti, e una sessione: (assistente, sessione).
    async fn assistente(&self, provider: Option<&str>, macchina: &str) -> (String, String) {
        let mut corpo = json!({ "nome": format!("Postino {}", xbot_core::nuovo_id()),
                                "titolo": "Posta", "macchina_predefinita_id": macchina });
        if let Some(p) = provider {
            corpo["provider_predefinito_id"] = json!(p);
        }
        let (s, a) = self.chiama("POST", "/api/v1/assistenti", Some(corpo)).await;
        assert_eq!(s, StatusCode::OK, "{a}");
        let aid = a["id"].as_str().unwrap().to_string();
        let (s, sess) = self
            .chiama(
                "POST",
                &format!("/api/v1/assistenti/{aid}/sessioni"),
                Some(json!({})),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{sess}");
        assert_eq!(sess["macchina_id"], macchina, "{sess}");
        (aid, sess["id"].as_str().unwrap().to_string())
    }

    fn turno(&self, sid: &str) -> (String, Option<String>) {
        self.p
            .db
            .con(|c| {
                Ok(c.query_row(
                    "SELECT stato, errore_codice FROM turno WHERE sessione_id = ?1
                     ORDER BY iniziato_ms DESC, rowid DESC LIMIT 1",
                    [sid],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap())
            })
            .unwrap()
    }

    async fn aspetta_turno(&self, sid: &str) -> (String, Option<String>) {
        for _ in 0..300 {
            let t = self.turno(sid);
            if t.0 != "in_corso" {
                return t;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("turno ancora in corso");
    }

    fn righe(&self, sid: &str) -> Vec<Value> {
        self.p
            .db
            .con(|c| {
                xbot_db::tutte_json(
                    c,
                    "SELECT genere, ruolo, corpo_json FROM riga WHERE sessione_id = ?1 ORDER BY seq",
                    [sid],
                )
            })
            .unwrap()
    }

    fn testo_assistente(&self, sid: &str) -> String {
        self.righe(sid)
            .iter()
            .filter(|r| r["genere"] == "messaggio" && r["ruolo"] == "assistente")
            .filter_map(|r| r["corpo_json"]["testo"].as_str().map(str::to_string))
            .collect::<Vec<_>>()
            .join("|")
    }
}

async fn ricevi(s: &mut Socket) -> Value {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(5), s.next())
            .await
            .expect("nessun frame entro 5 s")
            .expect("socket chiuso")
            .unwrap();
        if let Message::Text(t) = m {
            return serde_json::from_str(&t).unwrap();
        }
    }
}

/// Il prossimo frame del tipo dato (gli altri si saltano).
async fn aspetta(s: &mut Socket, tipo: &str) -> Value {
    loop {
        let v = ricevi(s).await;
        if v["tipo"] == tipo {
            return v;
        }
    }
}

async fn manda(s: &mut Socket, v: Value) {
    s.send(Message::Text(v.to_string().into())).await.unwrap();
}

async fn manda_ok(s: &mut Socket, lid: &str, seq: i64, v: Value) {
    let mut v = v;
    v["lavoro_id"] = json!(lid);
    v["seq"] = json!(seq);
    manda(s, v).await;
    let ok = aspetta(s, "ok").await;
    assert_eq!(
        (ok["lavoro_id"].as_str(), ok["fino_a_seq"].as_i64()),
        (Some(lid), Some(seq))
    );
}

fn testo(t: &str) -> Value {
    json!({ "tipo": "evento", "evento": { "tipo": "testo_delta", "testo": t } })
}

fn finito() -> Value {
    json!({ "tipo": "evento", "evento": { "tipo": "finito", "token_ingresso": 3, "token_uscita": 2,
            "token_cache": null, "costo_usd": null, "sessione_esterna_id": "chat-1" } })
}

#[tokio::test]
async fn collega_lavoro_ripresa_e_rifiuto() {
    let b = banco().await;
    let (mid, cred) = b.collega().await;

    // Elenchi con provider e macchine.
    let (_, m) = b.chiama("GET", "/api/v1/macchine", None).await;
    let mac = m["macchine"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == mid.as_str())
        .unwrap()
        .clone();
    assert_eq!(mac["provider"].as_array().unwrap().len(), 2, "{mac}");
    assert_eq!(mac["capacita"][1], "chrome");
    let (_, pv) = b.chiama("GET", "/api/v1/provider", None).await;
    let cur = pv
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "cursor_agent")
        .unwrap()
        .clone();
    assert_eq!(cur["macchine"][0]["id"], mid.as_str(), "{cur}");
    let (_, mo) = b
        .chiama("GET", "/api/v1/modelli?provider=cursor_agent", None)
        .await;
    assert_eq!(mo[0]["id"], "grok-4.7");
    assert_eq!(mo[0]["macchine"][0]["nome"], "mac");

    // Credenziale sbagliata: niente canale.
    assert!(b.apri(&mid, "sbagliata").await.is_err());
    let mut ws = b.apri(&mid, &cred).await.unwrap();
    let ciao = aspetta(&mut ws, "ciao").await;
    assert_eq!(ciao["macchina_id"], mid.as_str());
    manda(
        &mut ws,
        json!({ "tipo": "battito", "posti_liberi": 2, "provider": provider_mac(), "lavori": [] }),
    )
    .await;

    // Turno «Posta con Grok» sul Mac.
    let (_, sid) = b.assistente(Some("cursor_agent"), &mid).await;
    b.p.invia(&b.cookie, &sid, "i1", "leggi la posta").await;
    let l = aspetta(&mut ws, "lavoro").await;
    assert_eq!(l["richiesta"]["provider_id"], "cursor_agent");
    assert_eq!(l["richiesta"]["testo"], "leggi la posta");
    assert_eq!(l["generazione"], 1);
    assert_eq!(l["recinto_id"], sid.as_str());
    let lid = l["id"].as_str().unwrap().to_string();
    manda_ok(
        &mut ws,
        &lid,
        1,
        json!({ "tipo": "preso", "generazione": 1 }),
    )
    .await;
    manda_ok(&mut ws, &lid, 2, testo("ciao ")).await;
    manda_ok(&mut ws, &lid, 3, testo("Mario")).await;
    manda_ok(&mut ws, &lid, 4, finito()).await;
    manda_ok(
        &mut ws,
        &lid,
        5,
        json!({ "tipo": "chiuso", "stato": "riuscito" }),
    )
    .await;
    assert_eq!(b.aspetta_turno(&sid).await.0, "riuscito");
    assert_eq!(b.testo_assistente(&sid), "ciao Mario");
    assert!(
        b.righe(&sid)
            .iter()
            .any(|r| r["corpo_json"]["codice"] == "permessi_della_macchina")
    );
    let stato_lavoro = b.p.db.con(|c| xbot_db::lavori::leggi(c, &lid)).unwrap();
    assert_eq!(stato_lavoro["stato"], "riuscito");

    // Il battito con un elenco più corto: i modelli spariti si tolgono (nascosti, non più
    // disponibili), quelli nuovi si aggiungono; i modelli del server restano.
    let (_, finti_prima) = b
        .chiama("GET", "/api/v1/modelli?provider=finto", None)
        .await;
    let corto = json!([
        { "id": "cursor_agent", "nome": "Cursor", "programma": "/usr/local/bin/agent",
          "modelli": [ { "id": "grok-4.7-low", "etichetta": "Grok 4.7 Low (Cursor)" } ] },
        { "id": "codex", "nome": "Codex / ChatGPT", "programma": "codex", "modelli": [] }
    ]);
    manda(
        &mut ws,
        json!({ "tipo": "battito", "posti_liberi": 2, "provider": corto, "lavori": [] }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (_, mo) = b
        .chiama("GET", "/api/v1/modelli?provider=cursor_agent", None)
        .await;
    let ids: Vec<&str> = mo
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec!["grok-4.7-low"]);
    let (_, mo) = b
        .chiama("GET", "/api/v1/modelli?provider=codex", None)
        .await;
    assert!(mo.as_array().unwrap().is_empty(), "{mo}");
    let (_, m) = b.chiama("GET", "/api/v1/macchine", None).await;
    let mac = m["macchine"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == mid.as_str())
        .unwrap()
        .clone();
    let cur = mac["provider"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "cursor_agent")
        .unwrap()
        .clone();
    assert_eq!(cur["modelli"].as_array().unwrap().len(), 1, "{cur}");
    let tolto =
        b.p.db
            .con(|c| xbot_db::provider::verifica_modello(c, "cursor_agent", "grok-4.7"));
    assert_eq!(
        tolto.unwrap_err().codice,
        xbot_core::Codice::ModelloNonDisponibile
    );
    let (_, finti_dopo) = b
        .chiama("GET", "/api/v1/modelli?provider=finto", None)
        .await;
    assert_eq!(finti_prima, finti_dopo);
    assert!(!finti_dopo.as_array().unwrap().is_empty());

    // Ripresa: il socket cade a metà, il giornale rimanda (un evento già confermato e i nuovi).
    b.p.invia(&b.cookie, &sid, "i2", "ancora").await;
    let l = aspetta(&mut ws, "lavoro").await;
    assert_eq!(l["generazione"], 2);
    let lid = l["id"].as_str().unwrap().to_string();
    manda_ok(
        &mut ws,
        &lid,
        1,
        json!({ "tipo": "preso", "generazione": 2 }),
    )
    .await;
    manda_ok(&mut ws, &lid, 2, testo("a")).await;
    drop(ws);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(b.turno(&sid).0, "in_corso");
    let stato_mac = b.p.db.con(|c| xbot_db::macchine::leggi(c, &mid)).unwrap();
    assert_eq!(stato_mac["stato"], "riconnessione");
    let mut ws = b.apri(&mid, &cred).await.unwrap();
    let ciao = aspetta(&mut ws, "ciao").await;
    assert_eq!(ciao["lavori_aperti"][0]["lavoro_id"], lid.as_str());
    assert_eq!(ciao["lavori_aperti"][0]["fino_a_seq"], 2);
    manda_ok(&mut ws, &lid, 2, testo("a")).await; // doppione: confermato, non applicato
    manda_ok(&mut ws, &lid, 3, testo("b")).await;
    manda_ok(&mut ws, &lid, 4, finito()).await;
    manda_ok(
        &mut ws,
        &lid,
        5,
        json!({ "tipo": "chiuso", "stato": "riuscito" }),
    )
    .await;
    assert_eq!(b.aspetta_turno(&sid).await.0, "riuscito");
    assert_eq!(b.testo_assistente(&sid), "ciao Mario|ab");

    // Generazione vecchia: il lavoratore rifiuta, il turno fallisce `lavoro_rifiutato`.
    b.p.invia(&b.cookie, &sid, "i3", "terzo").await;
    let l = aspetta(&mut ws, "lavoro").await;
    let lid = l["id"].as_str().unwrap().to_string();
    manda_ok(
        &mut ws,
        &lid,
        1,
        json!({ "tipo": "chiuso", "stato": "rifiutato", "motivo": "generazione vecchia" }),
    )
    .await;
    let t = b.aspetta_turno(&sid).await;
    assert_eq!(
        (t.0.as_str(), t.1.as_deref()),
        ("fallito", Some("lavoro_rifiutato"))
    );

    // Un lavoro di un'altra macchina non si tocca.
    manda(
        &mut ws,
        json!({ "tipo": "evento", "lavoro_id": "nessuno", "seq": 1,
                           "evento": { "tipo": "avviso", "testo": "x" } }),
    )
    .await;
    assert_eq!(
        aspetta(&mut ws, "errore").await["codice"],
        "lavoro_non_trovato"
    );

    // Interrompere manda `interrompi`.
    b.p.invia(&b.cookie, &sid, "i4", "quarto").await;
    let l = aspetta(&mut ws, "lavoro").await;
    let lid = l["id"].as_str().unwrap().to_string();
    manda_ok(
        &mut ws,
        &lid,
        1,
        json!({ "tipo": "preso", "generazione": l["generazione"] }),
    )
    .await;
    let tid = l["turno_id"].as_str().unwrap().to_string();
    let (s, v) = b
        .chiama("POST", &format!("/api/v1/turni/{tid}/interrompi"), None)
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let f = aspetta(&mut ws, "interrompi").await;
    assert_eq!(f["lavoro_id"], lid.as_str());
    assert_eq!(b.aspetta_turno(&sid).await.0, "interrotto");

    // Rimozione: credenziale revocata.
    let (s, _) = b
        .chiama("DELETE", &format!("/api/v1/macchine/{mid}"), None)
        .await;
    assert_eq!(s, StatusCode::OK);
    assert!(b.apri(&mid, &cred).await.is_err());
    let (s, _) = b
        .chiama(
            "DELETE",
            &format!("/api/v1/macchine/{}", b.p.stato.servizi.macchina.id),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn attesa_del_mac_ripiego_e_lease() {
    let b = banco().await;
    b.p.stato.motore.macchine().imposta_tempi(Tempi {
        lease_ms: 200,
        persa_ms: 1500,
        attesa_max_ms: 60_000,
        push_dopo_ms: 300,
        giro_ms: 50,
    });
    let (mid, cred) = b.collega().await;

    // Client fisso sul Mac spento: il lavoro aspetta, parte quando il Mac si ricollega.
    let (_, sid) = b.assistente(Some("codex"), &mid).await;
    b.p.invia(&b.cookie, &sid, "a1", "ciao").await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(b.turno(&sid).0, "in_corso");
    assert!(
        b.righe(&sid)
            .iter()
            .any(|r| r["genere"] == "avviso" && r["corpo_json"]["codice"] == "in_attesa_macchina"),
        "{:?}",
        b.righe(&sid)
    );
    let (eventi, notifiche) =
        b.p.db
            .con(|c| {
                let e: i64 = c
                    .query_row(
                        "SELECT COUNT(*) FROM evento_sistema WHERE tipo = 'lavoro_in_attesa'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                let n: i64 = c
                    .query_row("SELECT COUNT(*) FROM notifica", [], |r| r.get(0))
                    .unwrap();
                Ok((e, n))
            })
            .unwrap();
    assert_eq!((eventi, notifiche), (1, 1));
    let mut ws = b.apri(&mid, &cred).await.unwrap();
    let l = aspetta(&mut ws, "lavoro").await;
    assert_eq!(l["richiesta"]["provider_id"], "codex");
    let lid = l["id"].as_str().unwrap().to_string();
    manda_ok(
        &mut ws,
        &lid,
        1,
        json!({ "tipo": "preso", "generazione": 1 }),
    )
    .await;

    // Una richiesta di permesso dal Mac: va all'utente, la risposta torna con l'id del lavoratore.
    manda_ok(
        &mut ws,
        &lid,
        2,
        json!({ "tipo": "richiesta", "richiesta_id": "loc-1", "tipo_richiesta": "revisione_comando",
                "carico": { "comando": "ls", "strumento": "Bash" } }),
    )
    .await;
    let (_, rich) = b.chiama("GET", "/api/v1/richieste", None).await;
    let r = rich
        .as_array()
        .or(rich["richieste"].as_array())
        .unwrap()
        .iter()
        .find(|r| r["carico_json"]["lavoro_id"] == lid.as_str())
        .unwrap()
        .clone();
    assert_eq!(r["tipo"], "revisione_comando");
    let rid = r["id"].as_str().unwrap();
    let (s, v) = b
        .chiama(
            "POST",
            &format!("/api/v1/richieste/{rid}/risolvi"),
            Some(json!({ "esito": "approvata", "comando_approvato": "ls" })),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let ris = aspetta(&mut ws, "risolta").await;
    assert_eq!(
        (
            ris["richiesta_id"].as_str(),
            ris["esito"].as_str(),
            ris["comando_approvato"].as_str()
        ),
        (Some("loc-1"), Some("approvata"), Some("ls"))
    );

    // Lease: il Mac smette di battere, dopo `persa_ms` il turno fallisce `macchina_persa`.
    let t = b.aspetta_turno(&sid).await;
    assert_eq!(
        (t.0.as_str(), t.1.as_deref()),
        ("fallito", Some("macchina_persa"))
    );
    drop(ws);
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Auto con il Mac spento: ripiega qui (finto) con l'avviso.
    let (_, sid2) = b.assistente(None, &mid).await;
    b.p.invia(&b.cookie, &sid2, "a2", "ciao").await;
    assert_eq!(b.aspetta_turno(&sid2).await.0, "riuscito");
    let righe = b.righe(&sid2);
    let avviso = righe
        .iter()
        .find(|r| r["corpo_json"]["codice"] == "ripiego_vps")
        .unwrap();
    assert!(
        avviso["corpo_json"]["testo"]
            .as_str()
            .unwrap()
            .starts_with("mac spento: eseguo sulla VPS")
    );
    let macchina_turno: Option<String> =
        b.p.db
            .con(|c| {
                Ok(c.query_row(
                    "SELECT macchina_id FROM turno WHERE sessione_id = ?1",
                    [&sid2],
                    |r| r.get(0),
                )
                .unwrap())
            })
            .unwrap();
    assert_eq!(
        macchina_turno.as_deref(),
        Some(b.p.stato.servizi.macchina.id.as_str())
    );

    // La riga dell'attesa del primo turno è chiusa: il Mac l'aveva preso.
    let chiusa = b
        .righe(&sid)
        .into_iter()
        .find(|r| r["corpo_json"]["codice"] == "in_attesa_macchina")
        .unwrap();
    assert_eq!(chiusa["corpo_json"]["risolto"], true);
    assert_eq!(chiusa["corpo_json"]["stato"], "ripartito");

    // «Esegui sulla VPS» su un turno in attesa: riparte qui (finto, manca claude_code).
    let (_, sid3) = b.assistente(Some("codex"), &mid).await;
    let inv = b.p.invia(&b.cookie, &sid3, "a3", "subito").await;
    let tid = inv["turno_id"].as_str().unwrap().to_string();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let (s, t) = b
        .chiama("POST", &format!("/api/v1/turni/{tid}/sulla-vps"), None)
        .await;
    assert_eq!(s, StatusCode::OK, "{t}");
    assert_eq!(t["provider_id"], "finto", "{t}");
    assert_eq!(b.aspetta_turno(&sid3).await.0, "riuscito");
    let attesa = b
        .righe(&sid3)
        .into_iter()
        .find(|r| r["corpo_json"]["codice"] == "in_attesa_macchina")
        .unwrap();
    assert_eq!(attesa["corpo_json"]["stato"], "ripartito");
    let (s, _) = b
        .chiama("POST", &format!("/api/v1/turni/{tid}/sulla-vps"), None)
        .await;
    assert_eq!(s, StatusCode::CONFLICT);

    // Squadra e scarica.
    let (s, sq) = b.chiama("GET", "/api/v1/squadra", None).await;
    assert_eq!(s, StatusCode::OK);
    let codex = sq["assistenti"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["provider"]["id"] == "codex")
        .unwrap()
        .clone();
    assert_eq!(codex["macchina"]["nome"], "mac", "{codex}");
    assert_eq!(codex["titolo"], "Posta");
    assert!(codex["ultimo_turno_ms"].is_i64());
    let (s, _) = b
        .chiama("GET", "/api/v1/scarica/xbot-worker-macos", None)
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn testo_di_sistema_con_la_squadra() {
    let b = banco().await;
    let (mid, _) = b.collega().await;
    let (aid, sid) = b.assistente(Some("cursor_agent"), &mid).await;
    b.assistente(None, &mid).await;
    let (s, _) = b
        .chiama(
            "PATCH",
            &format!("/api/v1/assistenti/{aid}"),
            Some(json!({ "modello_predefinito_id": "grok-4.7", "usa_chrome": 1 })),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let t =
        b.p.db
            .con(|c| {
                xbot_server::sistema::componi_sync(c, std::path::Path::new("/nessuna"), &sid, None)
            })
            .unwrap()
            .unwrap();
    assert!(t.contains("Tu sei Postino"), "{t}");
    assert!(
        t.contains("lavori con Cursor/Grok 4.7 (Cursor) su mac"),
        "{t}"
    );
    assert!(t.contains("lavora con Auto su mac"), "{t}");
}

// ───────────────────────── Fase 2.E-bis ─────────────────────────

/// Un allegato caricato nella sessione: l'id.
async fn carica(b: &Banco, sid: &str, nome: &str, byte: &[u8]) -> String {
    use axum::body::Body;
    use axum::http::{Request, header};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let confine = "----confine-xbot";
    let mut corpo = format!(
        "--{confine}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{nome}\"\r\nContent-Type: text/plain\r\n\r\n"
    )
    .into_bytes();
    corpo.extend_from_slice(byte);
    corpo.extend_from_slice(format!("\r\n--{confine}--\r\n").as_bytes());
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/sessioni/{sid}/allegati"))
        .header(header::COOKIE, &b.cookie)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={confine}"),
        )
        .body(Body::from(corpo))
        .unwrap();
    let r = b.p.app.clone().oneshot(req).await.unwrap();
    let stato = r.status();
    let v: Value =
        serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(stato, StatusCode::OK, "{v}");
    v["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn chiamata_vocale_e_voto_sul_server() {
    let b = banco().await;
    let server = b.p.stato.servizi.macchina.id.clone();
    let (_, sid) = b.assistente(None, &server).await;
    let (_, sid2) = b.assistente(None, &server).await;

    let uri = format!("/api/v1/sessioni/{sid}/chiamate");
    let corpo = json!({ "iniziata_ms": 1000, "durata_ms": 65_000, "invii": 3, "risposte": 2 });
    let (s, v) = b.chiama("POST", &uri, Some(corpo)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let rid = v["riga_id"].as_str().unwrap().to_string();
    let riga = b
        .righe(&sid)
        .into_iter()
        .find(|r| r["genere"] == "evento")
        .unwrap();
    assert_eq!(
        riga["corpo_json"],
        json!({ "tipo": "chiamata_vocale", "iniziata_ms": 1000, "durata_ms": 65_000,
                "invii": 3, "risposte": 2, "voto": null })
    );
    // Errori della creazione: sessione che non c'è, campi sbagliati.
    let (s, _) = b
        .chiama(
            "POST",
            "/api/v1/sessioni/nessuna/chiamate",
            Some(json!({ "iniziata_ms": 1, "durata_ms": 1 })),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = b
        .chiama(
            "POST",
            &uri,
            Some(json!({ "iniziata_ms": 1, "durata_ms": -1 })),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Voto sulla riga della chiamata: riscritta nel flusso (aggiornata_seq cresce).
    let (_, prima) = b
        .chiama("GET", &format!("/api/v1/sessioni/{sid}/righe"), None)
        .await;
    let cursore = prima["ultimo_aggiornata_seq"].as_i64().unwrap();
    let (s, v) = b
        .chiama(
            "POST",
            &format!("/api/v1/righe/{rid}/voto"),
            Some(json!({ "voto": "su" })),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["voto"], "su");
    let (_, dopo) = b
        .chiama(
            "GET",
            &format!("/api/v1/sessioni/{sid}/righe?dopo_aggiornata_seq={cursore}"),
            None,
        )
        .await;
    let nuove = dopo["righe"].as_array().unwrap();
    assert_eq!(nuove.len(), 1, "{dopo}");
    assert_eq!(nuove[0]["id"], rid.as_str());
    assert_eq!(nuove[0]["corpo"]["voto"], "su");
    assert_eq!(nuove[0]["corpo"]["durata_ms"], 65_000);
    let (s, v) = b
        .chiama(
            "POST",
            &format!("/api/v1/sessioni/{sid}/righe/{rid}/voto"),
            Some(json!({ "voto": null })),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["voto"].is_null());

    // Voto su un messaggio dell'assistente (turno finto sul server).
    b.p.invia(&b.cookie, &sid, "v1", "ciao").await;
    assert_eq!(b.aspetta_turno(&sid).await.0, "riuscito");
    let (_, pag) = b
        .chiama("GET", &format!("/api/v1/sessioni/{sid}/righe"), None)
        .await;
    let righe = pag["righe"].as_array().unwrap();
    let risposta = righe
        .iter()
        .find(|r| r["genere"] == "messaggio" && r["ruolo"] == "assistente")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let utente = righe
        .iter()
        .find(|r| r["genere"] == "messaggio" && r["ruolo"] == "utente")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (s, v) = b
        .chiama(
            "POST",
            &format!("/api/v1/righe/{risposta}/voto"),
            Some(json!({ "voto": "giu" })),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let corpo = b
        .righe(&sid)
        .into_iter()
        .find(|r| r["genere"] == "messaggio" && r["ruolo"] == "assistente")
        .unwrap()["corpo_json"]
        .clone();
    assert_eq!(corpo["voto"], "giu");
    assert!(corpo["testo"].is_string(), "il testo resta: {corpo}");

    // Errori del voto.
    let bb = &b;
    let voto = |uri: String, c: Value| async move { bb.chiama("POST", &uri, Some(c)).await.0 };
    assert_eq!(
        voto(
            format!("/api/v1/sessioni/{sid2}/righe/{rid}/voto"),
            json!({ "voto": "su" })
        )
        .await,
        StatusCode::NOT_FOUND,
        "riga di un'altra sessione"
    );
    assert_eq!(
        voto("/api/v1/righe/nessuna/voto".into(), json!({ "voto": "su" })).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        voto(
            format!("/api/v1/righe/{rid}/voto"),
            json!({ "voto": "forse" })
        )
        .await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        voto(format!("/api/v1/righe/{rid}/voto"), json!({})).await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        voto(
            format!("/api/v1/righe/{utente}/voto"),
            json!({ "voto": "su" })
        )
        .await,
        StatusCode::BAD_REQUEST,
        "un messaggio di Mario non si vota"
    );
}

#[tokio::test]
async fn risolta_in_coda_e_allegati_nel_lavoro() {
    let b = banco().await;
    let (mid, cred) = b.collega().await;
    let mut ws = b.apri(&mid, &cred).await.unwrap();
    aspetta(&mut ws, "ciao").await;
    let (_, sid) = b.assistente(Some("codex"), &mid).await;

    // Allegato nel frame `lavoro`: contenuto in base64, nel carico salvato solo il nome.
    let aid = carica(&b, &sid, "nota.txt", b"ciao dal server").await;
    let (s, v, _) =
        b.p.chiama(
            "POST",
            &format!("/api/v1/sessioni/{sid}/invii"),
            Some(
                json!({ "id": "01J0000000000000000000INVA", "testo": "leggi", "allegati": [aid] }),
            ),
            Some(&b.cookie),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let l = aspetta(&mut ws, "lavoro").await;
    let al = &l["richiesta"]["allegati"][0];
    assert_eq!(al["nome"], "nota.txt", "{l}");
    assert_eq!(al["tipo_mime"], "text/plain");
    assert_eq!(al["dimensione"], 15);
    assert_eq!(al["contenuto_base64"], "Y2lhbyBkYWwgc2VydmVy");
    assert_eq!(
        l["richiesta"]["testo"], "leggi",
        "niente nota «rimasto sul server»"
    );
    let lid = l["id"].as_str().unwrap().to_string();
    let carico = b.p.db.con(|c| xbot_db::lavori::leggi(c, &lid)).unwrap();
    assert!(
        !carico.to_string().contains("Y2lhbyBkYWwgc2VydmVy"),
        "{carico}"
    );

    // Richiesta dal Mac, socket tagliato, l'utente risponde: la risolta aspetta la riconnessione.
    manda_ok(
        &mut ws,
        &lid,
        1,
        json!({ "tipo": "preso", "generazione": 1 }),
    )
    .await;
    manda_ok(
        &mut ws,
        &lid,
        2,
        json!({ "tipo": "richiesta", "richiesta_id": "loc-9", "tipo_richiesta": "revisione_comando",
                "carico": { "comando": "touch x", "strumento": "Bash" } }),
    )
    .await;
    drop(ws);
    tokio::time::sleep(Duration::from_millis(150)).await;
    let (_, rich) = b.chiama("GET", "/api/v1/richieste", None).await;
    let rid = rich
        .as_array()
        .or(rich["richieste"].as_array())
        .unwrap()
        .iter()
        .find(|r| r["carico_json"]["lavoro_id"] == lid.as_str())
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (s, v) = b
        .chiama(
            "POST",
            &format!("/api/v1/richieste/{rid}/risolvi"),
            Some(json!({ "esito": "approvata", "comando_approvato": "touch x" })),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    tokio::time::sleep(Duration::from_millis(150)).await;
    let in_coda: i64 =
        b.p.db
            .con(|c| {
                Ok(c.query_row(
                    "SELECT COUNT(*) FROM risolta_in_coda WHERE consegnata_ms IS NULL",
                    [],
                    |r| r.get(0),
                )
                .unwrap())
            })
            .unwrap();
    assert_eq!(in_coda, 1);

    // Riconnessione: ciao, il lavoro aperto (già preso: non si rimanda), poi la risolta.
    let mut ws = b.apri(&mid, &cred).await.unwrap();
    let ciao = aspetta(&mut ws, "ciao").await;
    assert_eq!(ciao["lavori_aperti"][0]["lavoro_id"], lid.as_str());
    let ris = aspetta(&mut ws, "risolta").await;
    assert_eq!(
        (
            ris["richiesta_id"].as_str(),
            ris["esito"].as_str(),
            ris["comando_approvato"].as_str()
        ),
        (Some("loc-9"), Some("approvata"), Some("touch x"))
    );
    // Una volta sola: alla riconnessione dopo non torna.
    drop(ws);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut ws = b.apri(&mid, &cred).await.unwrap();
    aspetta(&mut ws, "ciao").await;
    manda(
        &mut ws,
        json!({ "tipo": "battito", "posti_liberi": 1, "provider": provider_mac(), "lavori": [lid] }),
    )
    .await;
    loop {
        match tokio::time::timeout(Duration::from_millis(400), ws.next()).await {
            Err(_) => break,
            Ok(Some(Ok(Message::Text(t)))) => {
                let v: Value = serde_json::from_str(&t).unwrap();
                assert_ne!(v["tipo"], "risolta", "risolta mandata due volte");
            }
            Ok(_) => break,
        }
    }
    manda_ok(&mut ws, &lid, 3, finito()).await;
    manda_ok(
        &mut ws,
        &lid,
        4,
        json!({ "tipo": "chiuso", "stato": "riuscito" }),
    )
    .await;
    assert_eq!(b.aspetta_turno(&sid).await.0, "riuscito");
}
