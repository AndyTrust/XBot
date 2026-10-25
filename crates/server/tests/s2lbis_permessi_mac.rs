//! Prove della 2.L-bis (S): le richieste di permesso del Mac passano dai permessi per strumento
//! dei collegamenti (e da concessioni e regole) prima di arrivare all'utente; i nomi dei
//! collegamenti sul filo sono quelli puliti del `--mcp-config`.

mod comune;
mod s2_comune;
mod s2k_comune;

use comune::*;
use futures::SinkExt;
use s2k_comune::*;
use tokio_tungstenite::tungstenite::Message;
use xbot_db::collegamenti::StrumentoNoto;

async fn manda(ws: &mut Socket, v: Value) {
    ws.send(Message::Text(v.to_string().into())).await.unwrap();
}

async fn richieste_del_lavoro(p: &Prova, c: &str, lid: &str) -> Vec<Value> {
    let (_, rich) = chiama(p, c, "GET", "/api/v1/richieste", None).await;
    rich.as_array()
        .or(rich["richieste"].as_array())
        .unwrap()
        .iter()
        .filter(|r| r["carico_json"]["lavoro_id"] == lid)
        .cloned()
        .collect()
}

#[tokio::test]
async fn permessi_per_strumento_anche_dal_mac() {
    let (p, c, indirizzo) = banco(None).await;
    let (mid, mut ws) = mac(&p, &c, &indirizzo).await;
    let id = crea(
        &p,
        &c,
        json!({ "nome": "Posta (IMAP)", "trasporto": "stdio", "comando": "python3",
                "argomenti": ["~/strumenti/posta_mcp.py"], "macchina_id": mid }),
    )
    .await;
    let id2 = id.clone();
    p.db.con(move |k| {
        let s = [
            StrumentoNoto {
                nome: "caselle".into(),
                descrizione: None,
                sola_lettura: true,
            },
            StrumentoNoto {
                nome: "invia".into(),
                descrizione: None,
                sola_lettura: false,
            },
        ];
        xbot_db::collegamenti::segna_prova(k, &id2, Ok(&s))
    })
    .unwrap();
    let uri = format!("/api/v1/collegamenti/{id}/permessi");
    let corpo = json!({ "permessi": { "*lettura": "sempre", "invia": "bloccato" } });
    let (s, v) = chiama(&p, &c, "PUT", &uri, Some(corpo)).await;
    assert_eq!(s, StatusCode::OK, "{v}");

    let corpo = json!({ "nome": "Postino", "provider_predefinito_id": "claude_code",
                        "macchina_predefinita_id": mid });
    let (_, a) = chiama(&p, &c, "POST", "/api/v1/assistenti", Some(corpo)).await;
    let aid = a["id"].as_str().unwrap();
    let uri = format!("/api/v1/assistenti/{aid}/sessioni");
    let (_, sess) = chiama(&p, &c, "POST", &uri, Some(json!({}))).await;
    let sid = sess["id"].as_str().unwrap().to_string();
    p.invia(&c, &sid, &xbot_core::nuovo_id(), "elenca le caselle")
        .await;
    let l = aspetta(&mut ws, "lavoro").await;
    let lid = l["id"].as_str().unwrap().to_string();
    // Il nome sul filo è quello pulito: Cursor rifiuta spazi e parentesi.
    assert_eq!(
        l["richiesta"]["collegamenti"][0]["nome"], "posta_imap",
        "{l}"
    );
    manda(
        &mut ws,
        json!({ "tipo": "preso", "lavoro_id": lid, "seq": 1, "generazione": 1 }),
    )
    .await;

    // Sola lettura con le letture «sempre»: approvata dal server, nessuna richiesta all'utente.
    let r = |seq: i64, rid: &str, strumento: &str, carico: Value| {
        let mut carico = carico;
        carico["strumento"] = json!(strumento);
        json!({ "tipo": "richiesta", "lavoro_id": lid, "seq": seq, "richiesta_id": rid,
                "tipo_richiesta": "permesso_locale", "carico": carico })
    };
    manda(
        &mut ws,
        r(
            2,
            "loc-1",
            "mcp__posta_imap__caselle",
            json!({ "input": {} }),
        ),
    )
    .await;
    let ris = aspetta(&mut ws, "risolta").await;
    assert_eq!(
        (ris["richiesta_id"].as_str(), ris["esito"].as_str()),
        (Some("loc-1"), Some("approvata")),
        "{ris}"
    );
    assert!(richieste_del_lavoro(&p, &c, &lid).await.is_empty());

    // Strumento bloccato: negato dal server con il motivo.
    manda(
        &mut ws,
        r(3, "loc-2", "mcp__posta_imap__invia", json!({ "input": {} })),
    )
    .await;
    let ris = aspetta(&mut ws, "risolta").await;
    assert_eq!(
        (ris["richiesta_id"].as_str(), ris["esito"].as_str()),
        (Some("loc-2"), Some("negata")),
        "{ris}"
    );
    assert!(
        ris["motivo"]
            .as_str()
            .unwrap_or_default()
            .contains("bloccato")
    );
    assert!(richieste_del_lavoro(&p, &c, &lid).await.is_empty());

    // Un comando senza concessione: va all'utente come prima.
    manda(
        &mut ws,
        r(
            4,
            "loc-3",
            "Bash",
            json!({ "comando": "ls", "input": { "command": "ls" } }),
        ),
    )
    .await;
    aspetta(&mut ws, "ok").await;
    let mut trovate = Vec::new();
    for _ in 0..100 {
        trovate = richieste_del_lavoro(&p, &c, &lid).await;
        if !trovate.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(trovate.len(), 1, "{trovate:?}");
    assert_eq!(trovate[0]["carico_json"]["strumento"], "Bash");
}

/// L'avviso «questo client non chiede i permessi…» esce una volta per sessione.
#[tokio::test]
async fn avviso_dei_permessi_una_volta_per_sessione() {
    let (p, c, indirizzo) = banco(None).await;
    let (mid, mut ws) = mac(&p, &c, &indirizzo).await;
    let corpo = json!({ "nome": "Grok", "provider_predefinito_id": "grok",
                        "macchina_predefinita_id": mid });
    let (_, a) = chiama(&p, &c, "POST", "/api/v1/assistenti", Some(corpo)).await;
    let aid = a["id"].as_str().unwrap();
    let uri = format!("/api/v1/assistenti/{aid}/sessioni");
    let (_, sess) = chiama(&p, &c, "POST", &uri, Some(json!({}))).await;
    let sid = sess["id"].as_str().unwrap().to_string();
    for giro in 0..2 {
        p.invia(&c, &sid, &xbot_core::nuovo_id(), "ciao").await;
        let l = aspetta(&mut ws, "lavoro").await;
        let lid = l["id"].as_str().unwrap().to_string();
        let g = l["generazione"].clone();
        manda(
            &mut ws,
            json!({ "tipo": "preso", "lavoro_id": lid, "seq": 1, "generazione": g }),
        )
        .await;
        let fine = json!({ "tipo": "finito", "token_ingresso": 1, "token_uscita": 1,
            "token_cache": null, "costo_usd": null, "sessione_esterna_id": "g-1" });
        manda(
            &mut ws,
            json!({ "tipo": "evento", "lavoro_id": lid, "seq": 2, "evento": fine }),
        )
        .await;
        manda(
            &mut ws,
            json!({ "tipo": "chiuso", "lavoro_id": lid, "seq": 3, "stato": "riuscito" }),
        )
        .await;
        let mut stato = String::new();
        for _ in 0..200 {
            let s2 = sid.clone();
            stato =
                p.db.con(move |k| {
                    Ok(k.query_row(
                        "SELECT stato FROM turno WHERE sessione_id = ?1
                         ORDER BY iniziato_ms DESC, rowid DESC LIMIT 1",
                        [s2],
                        |r| r.get::<_, String>(0),
                    )
                    .unwrap())
                })
                .unwrap();
            if stato != "in_corso" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(stato, "riuscito", "giro {giro}");
    }
    let righe = p.righe(&c, &sid, 0).await;
    let note = righe["righe"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["corpo"]["codice"] == "permessi_della_macchina")
        .count();
    assert_eq!(note, 1, "{righe}");
}
