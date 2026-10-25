//! Prove della 2.I: account dei client, «Entra» ed «Esci» sul server (client finto che stampa un
//! url e poi scrive il file di accesso) e sul Mac (lavoratore finto in-process), provider spento.

mod comune;

use comune::*;
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn script() -> String {
    format!(
        "{}/../provider/prove/accesso-finto",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn trova<'a>(v: &'a Value, id: &str) -> &'a Value {
    v.as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id)
        .unwrap_or_else(|| panic!("{id} non c'è in {v}"))
}

async fn provider(p: &Prova, c: &str) -> Value {
    let (s, v, _) = p.chiama("GET", "/api/v1/provider", None, Some(c)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

#[tokio::test]
async fn entra_ed_esci_sul_server() {
    let file = std::env::temp_dir().join(format!("xbot-accesso-{}", xbot_core::nuovo_id()));
    let finto = Finto::con_ritardo(Duration::from_millis(5)).con_accesso(script(), &file);
    let p = prova_con(vec![Arc::new(finto)]).await;
    let c = p.entra().await;

    let v = provider(&p, &c).await;
    let f = trova(&v, "finto");
    assert_eq!(f["riceve_collegamenti"], false);
    assert_eq!(f["quota_fonte"], "stimata");
    let m = &f["macchine"][0];
    assert_eq!(
        (m["e_il_server"].clone(), m["in_linea"].clone()),
        (json!(true), json!(true))
    );
    assert_eq!(m["account"], Value::Null, "{m}");

    let (s, v, _) = p
        .chiama(
            "POST",
            "/api/v1/provider/finto/accedi",
            Some(json!({})),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["modo"], "link");
    assert_eq!(v["url"], "https://esempio.it/accedi?x=1");
    assert_eq!(v["codice_utente"], "ABCD-1234");
    assert_eq!(v["scade_s"], 300);
    let mut stato = Value::Null;
    for _ in 0..100 {
        let (_, a, _) = p
            .chiama("GET", "/api/v1/provider/finto/accesso", None, Some(&c))
            .await;
        stato = a;
        if stato["stato"] != "in_attesa" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(stato["stato"], "riuscito", "{stato}");
    assert_eq!(stato["account"]["nome"], "finto@prova");
    let v = provider(&p, &c).await;
    assert_eq!(
        trova(&v, "finto")["macchine"][0]["account"]["nome"],
        "finto@prova"
    );

    let (s, v, _) = p
        .chiama(
            "POST",
            "/api/v1/provider/finto/esci",
            Some(json!({})),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        (v["stato"].clone(), v["account"].clone()),
        (json!("uscito"), Value::Null)
    );
    assert!(!file.exists());

    let (s, _, _) = p
        .chiama(
            "POST",
            "/api/v1/provider/nessuno/accedi",
            Some(json!({})),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Spento: `attivo = 0` e i suoi modelli fuori dalla scelta.
    let (s, v, _) = p
        .chiama(
            "PATCH",
            "/api/v1/provider/finto",
            Some(json!({ "attivo": false })),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(trova(&provider(&p, &c).await, "finto")["attivo"], 0);
    let (_, mo, _) = p.chiama("GET", "/api/v1/modelli", None, Some(&c)).await;
    assert_eq!(mo[0]["provider_attivo"], false, "{mo}");
}

async fn ricevi(s: &mut Socket, tipo: &str) -> Value {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(5), s.next())
            .await
            .expect("nessun frame entro 5 s")
            .expect("socket chiuso")
            .unwrap();
        if let Message::Text(t) = m {
            let v: Value = serde_json::from_str(&t).unwrap();
            if v["tipo"] == tipo {
                return v;
            }
        }
    }
}

async fn manda(s: &mut Socket, v: Value) {
    s.send(Message::Text(v.to_string().into())).await.unwrap();
}

#[tokio::test]
async fn account_nel_battito_ed_entra_sul_mac() {
    let p = prova(0).await;
    let c = p.entra().await;
    let ascolto = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let indirizzo = ascolto.local_addr().unwrap().to_string();
    let app = p.app.clone();
    tokio::spawn(async move { axum::serve(ascolto, app).await.unwrap() });

    let (_, b, _) = p
        .chiama("POST", "/api/v1/macchine/biglietto", None, Some(&c))
        .await;
    let cursor = |account: Value| {
        json!([{ "id": "cursor_agent", "nome": "Cursor", "programma": "/usr/local/bin/agent",
                 "versione": "2026.10.22", "account": account,
                 "modelli": [{ "id": "grok-4.7", "etichetta": "Grok 4.7" }] }])
    };
    let corpo = json!({
        "biglietto": b["biglietto"], "nome": "mac", "genere": "mac", "os": "macos",
        "arch": "aarch64", "versione_lavoratore": "0.1.0", "chiave_pubblica": "SPKI", "posti": 2,
        "capacita": ["turni"], "provider": cursor(Value::Null),
    });
    let (s, v, _) = p
        .chiama("POST", "/api/v1/macchine/collega", Some(corpo), None)
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let mid = v["macchina_id"].as_str().unwrap().to_string();
    let cred = v["credenziale"].as_str().unwrap().to_string();

    // Con il Mac spento: «Entra» risponde 409.
    let (s, _, _) = p
        .chiama(
            "POST",
            "/api/v1/provider/cursor_agent/accedi",
            Some(json!({ "macchina_id": mid })),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);

    let url = format!("ws://{indirizzo}/api/v1/macchine/{mid}/canale");
    let mut req = url.into_client_request().unwrap();
    req.headers_mut()
        .insert("authorization", format!("Bearer {cred}").parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    ricevi(&mut ws, "ciao").await;
    let account = json!({ "nome": "utente@mac.it", "dettaglio": null });
    manda(
        &mut ws,
        json!({ "tipo": "battito", "posti_liberi": 2,
                           "provider": cursor(account), "lavori": [] }),
    )
    .await;
    let mut m = Value::Null;
    for _ in 0..50 {
        let v = provider(&p, &c).await;
        m = trova(&v, "cursor_agent")["macchine"][0].clone();
        if m["account"].is_object() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(m["account"]["nome"], "utente@mac.it", "{m}");
    assert_eq!(
        (m["in_linea"].clone(), m["versione"].clone()),
        (json!(true), json!("2026.10.22"))
    );
    assert_eq!(m["e_il_server"], false);

    // «Entra»: il server chiede, il lavoratore risponde con l'url.
    let p2 = Arc::new(p);
    let (pp, cc, mm) = (p2.clone(), c.clone(), mid.clone());
    let attesa = tokio::spawn(async move {
        pp.chiama(
            "POST",
            "/api/v1/provider/cursor_agent/accedi",
            Some(json!({ "macchina_id": mm })),
            Some(&cc),
        )
        .await
    });
    let f = ricevi(&mut ws, "accedi").await;
    assert_eq!(f["provider_id"], "cursor_agent");
    manda(
        &mut ws,
        json!({ "tipo": "accesso", "richiesta_id": f["richiesta_id"],
        "provider_id": "cursor_agent", "stato": "in_attesa",
        "url": "https://cursor.com/loginDeepControl?x=1", "chiede_codice": false }),
    )
    .await;
    let (s, v, _) = attesa.await.unwrap();
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        (v["modo"].clone(), v["url"].clone()),
        (
            json!("link"),
            json!("https://cursor.com/loginDeepControl?x=1")
        )
    );
    manda(
        &mut ws,
        json!({ "tipo": "accesso", "provider_id": "cursor_agent", "stato": "riuscito",
        "account": { "nome": "utente@mac.it" } }),
    )
    .await;
    let mut a = Value::Null;
    for _ in 0..50 {
        let (_, x, _) = p2
            .chiama(
                "GET",
                &format!("/api/v1/provider/cursor_agent/accesso?macchina_id={mid}"),
                None,
                Some(&c),
            )
            .await;
        a = x;
        if a["stato"] == "riuscito" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(a["account"]["nome"], "utente@mac.it", "{a}");

    // «Esci».
    let (pp, cc, mm) = (p2.clone(), c.clone(), mid.clone());
    let attesa = tokio::spawn(async move {
        pp.chiama(
            "POST",
            "/api/v1/provider/cursor_agent/esci",
            Some(json!({ "macchina_id": mm })),
            Some(&cc),
        )
        .await
    });
    let f = ricevi(&mut ws, "esci").await;
    manda(
        &mut ws,
        json!({ "tipo": "accesso", "richiesta_id": f["richiesta_id"],
        "provider_id": "cursor_agent", "stato": "uscito", "account": null }),
    )
    .await;
    let (s, v, _) = attesa.await.unwrap();
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["stato"], "uscito");
}
