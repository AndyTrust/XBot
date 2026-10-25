//! Aiuti delle prove della 2.K: server con cassaforte e porta vera, Mac collegato con il canale,
//! lavoratore finto che risponde a `prova_collegamento` con il codice vero (`prova_mcp`).
#![allow(dead_code)]

use crate::comune::*;
use crate::s2_comune::FRASE;
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use xbot_provider::prova_mcp::{self, ServerDaProvare};
use xbot_server::servizi::OpzioniServizi;

pub type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub fn mcp_finto() -> String {
    format!("{}/tests/mcp-finto.py", env!("CARGO_MANIFEST_DIR"))
}

/// Un server con cassaforte e `XBOT_URL_PUBBLICO`, in ascolto su una porta vera (per il canale).
pub async fn banco(url: Option<&str>) -> (Prova, String, String) {
    let db = Db::apri(":memory:").unwrap();
    let segreto = base64::engine::general_purpose::STANDARD.encode([9u8; 32]);
    let mut o = OpzioniServizi::per_prove(Some(FRASE));
    o.url_pubblico = url.map(str::to_string);
    let stato = Stato::prepara_servizi(
        db.clone(),
        vec![Arc::new(Finto::con_ritardo(Duration::ZERO))],
        Some(SegretoTesto::nuovo(segreto)),
        false,
        Vec::new(),
        o,
    )
    .await
    .unwrap();
    let url_totp = accesso::impianta(
        &db,
        &stato.chiave_totp,
        "Mario",
        &SegretoTesto::nuovo(PASSWORD),
        true,
    )
    .unwrap()
    .unwrap();
    let p = Prova {
        app: app(stato.clone(), Path::new("web-che-non-esiste")),
        stato,
        db,
        url_totp,
    };
    let c = p.entra().await;
    let ascolto = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let indirizzo = ascolto.local_addr().unwrap().to_string();
    let a = p.app.clone();
    tokio::spawn(async move { axum::serve(ascolto, a).await.unwrap() });
    (p, c, indirizzo)
}

pub async fn chiama(
    p: &Prova,
    c: &str,
    m: &str,
    uri: &str,
    corpo: Option<Value>,
) -> (StatusCode, Value) {
    let (s, v, _) = p.chiama(m, uri, corpo, Some(c)).await;
    (s, v)
}

/// Un Mac collegato (con `claude_code`) e il suo canale aperto: (macchina_id, socket).
pub async fn mac(p: &Prova, c: &str, indirizzo: &str) -> (String, Socket) {
    let (_, b) = chiama(p, c, "POST", "/api/v1/macchine/biglietto", None).await;
    let prov = json!([
        { "id": "claude_code", "nome": "Claude Code", "programma": "claude",
          "modelli": [ { "id": "sonnet", "etichetta": "Sonnet" } ] },
        { "id": "grok", "nome": "Grok", "programma": "grok",
          "modelli": [ { "id": "grok-4.6", "etichetta": "Grok 4.6" } ] },
        { "id": "cursor_agent", "nome": "Cursor", "programma": "agent",
          "modelli": [ { "id": "composer-2", "etichetta": "Composer 2" } ] }
    ]);
    let corpo = json!({
        "biglietto": b["biglietto"], "nome": "mac", "genere": "mac", "os": "macos",
        "arch": "aarch64", "versione_lavoratore": "0.1.0", "chiave_pubblica": "SPKI",
        "posti": 2, "capacita": ["turni"], "provider": prov,
    });
    let (s, v, _) = p
        .chiama("POST", "/api/v1/macchine/collega", Some(corpo), None)
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let mid = v["macchina_id"].as_str().unwrap().to_string();
    let url = format!("ws://{indirizzo}/api/v1/macchine/{mid}/canale");
    let mut req = url.into_client_request().unwrap();
    let cred = v["credenziale"].as_str().unwrap();
    req.headers_mut()
        .insert("authorization", format!("Bearer {cred}").parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    aspetta(&mut ws, "ciao").await;
    let battito = json!({ "tipo": "battito", "posti_liberi": 2, "provider": prov, "lavori": [] });
    ws.send(Message::Text(battito.to_string().into()))
        .await
        .unwrap();
    (mid, ws)
}

pub async fn aspetta(s: &mut Socket, tipo: &str) -> Value {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(10), s.next())
            .await
            .expect("nessun frame entro 10 s")
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

/// Il lavoratore finto: risponde a un `prova_collegamento` con lo stesso codice del lavoratore
/// vero (`prova_mcp`) e restituisce il frame ricevuto.
pub async fn rispondi_a_una_prova(ws: &mut Socket) -> Value {
    let f = aspetta(ws, "prova_collegamento").await;
    let coppie = |v: &Value| -> Vec<(String, SegretoTesto)> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|c| {
                let n = c["nome"].as_str().unwrap().to_string();
                (n, SegretoTesto::nuovo(c["valore"].as_str().unwrap()))
            })
            .collect()
    };
    let c = &f["collegamento"];
    let srv = ServerDaProvare {
        trasporto: c["trasporto"].as_str().unwrap().into(),
        comando: c["comando"].as_str().map(str::to_string),
        argomenti: serde_json::from_value(c["argomenti"].clone()).unwrap(),
        ambiente: coppie(&c["ambiente"]),
        url: c["url"].as_str().map(str::to_string),
        intestazioni: coppie(&c["intestazioni"]),
    };
    let r = match prova_mcp::elenco_strumenti(&srv).await {
        Ok(t) => json!({ "tipo": "esito_prova", "richiesta_id": f["richiesta_id"], "ok": true,
                         "strumenti": t }),
        Err(e) => json!({ "tipo": "esito_prova", "richiesta_id": f["richiesta_id"], "ok": false,
                          "strumenti": [], "errore": e }),
    };
    ws.send(Message::Text(r.to_string().into())).await.unwrap();
    f
}

pub async fn crea(p: &Prova, c: &str, corpo: Value) -> String {
    let (s, v) = chiama(p, c, "POST", "/api/v1/collegamenti", Some(corpo)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v["id"].as_str().unwrap().to_string()
}

pub async fn prova_coll(p: &Prova, c: &str, id: &str) -> Value {
    let uri = format!("/api/v1/collegamenti/{id}/prova");
    let (s, v) = chiama(p, c, "POST", &uri, None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}
