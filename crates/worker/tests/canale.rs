//! Il protocollo del canale contro un server finto in-process: lavoro, eventi, chiusura,
//! conferma, generazione vecchia, ripresa dopo il taglio del socket.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::result_large_err
)]

use std::path::PathBuf;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use xbot_worker::Config;
use xbot_worker::credenziale::Credenziale;

type Ws = WebSocketStream<TcpStream>;

struct Banco {
    accettati: mpsc::Receiver<Ws>,
    cartella: PathBuf,
    _fermo: watch::Sender<bool>,
}

async fn banco() -> Banco {
    let ascolto = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let porta = ascolto.local_addr().unwrap().port();
    let (tx, accettati) = mpsc::channel(4);
    tokio::spawn(async move {
        loop {
            let (s, _) = ascolto.accept().await.unwrap();
            let controlla = |r: &Request, risp: Response| -> Result<Response, ErrorResponse> {
                let ok = r.uri().path() == "/api/v1/macchine/M1/canale"
                    && r.headers()
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        == Some("Bearer cred-di-prova");
                if ok {
                    Ok(risp)
                } else {
                    let mut e = ErrorResponse::new(None);
                    *e.status_mut() =
                        tokio_tungstenite::tungstenite::http::StatusCode::UNAUTHORIZED;
                    Err(e)
                }
            };
            if let Ok(ws) = tokio_tungstenite::accept_hdr_async(s, controlla).await {
                let _ = tx.send(ws).await;
            }
        }
    });
    let cartella = std::env::temp_dir().join(format!("plw-c-{}", &xbot_core::nuovo_id()[16..]));
    let conf = Config {
        cartella: cartella.clone(),
        posti: 2,
        consenti_finto: true,
        battito: Duration::from_millis(300),
        sessioni: cartella.join("sessioni"),
        programma: PathBuf::from(env!("CARGO_BIN_EXE_xbot-worker")),
        provider: Some(Vec::new()),
    };
    let cred = Credenziale {
        url: format!("http://127.0.0.1:{porta}"),
        macchina_id: "M1".into(),
        credenziale: "cred-di-prova".into(),
        chiave_privata: String::new(),
        chiave_pubblica: String::new(),
        nome: "mac".into(),
        creata_ms: 0,
    };
    let (fermo, rx) = watch::channel(false);
    tokio::spawn(async move {
        if let Err(e) = xbot_worker::canale::esegui(conf, cred, rx).await {
            eprintln!("lavoratore: {e}");
        }
    });
    Banco {
        accettati,
        cartella,
        _fermo: fermo,
    }
}

async fn manda(ws: &mut Ws, v: Value) {
    ws.send(Message::text(v.to_string())).await.unwrap();
}

/// Il prossimo frame che non è un battito.
async fn frame(ws: &mut Ws) -> Option<Value> {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .ok()??;
        let Ok(Message::Text(t)) = m else { continue };
        let v: Value = serde_json::from_str(t.as_str()).unwrap();
        if v["tipo"] != "battito" {
            return Some(v);
        }
    }
}

async fn battito(ws: &mut Ws) -> Value {
    loop {
        let m = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .unwrap()
            .unwrap();
        if let Ok(Message::Text(t)) = m {
            let v: Value = serde_json::from_str(t.as_str()).unwrap();
            if v["tipo"] == "battito" {
                return v;
            }
        }
    }
}

fn lavoro(id: &str, sessione: &str, generazione: i64, testo: &str) -> Value {
    json!({
        "tipo": "lavoro", "id": id, "generazione": generazione, "recinto_id": "R",
        "sessione_id": sessione, "turno_id": format!("T-{id}"),
        "richiesta": {"provider_id": "finto", "testo": testo,
            "ambiente": [{"nome": "SEGRETO_X", "valore": "non-nei-log"}]}
    })
}

#[tokio::test]
async fn lavoro_eventi_chiusura_e_recinto() {
    let mut b = banco().await;
    let mut ws = b.accettati.recv().await.unwrap();
    manda(
        &mut ws,
        json!({"tipo": "ciao", "macchina_id": "M1", "server_versione": "prova"}),
    )
    .await;
    let v = battito(&mut ws).await;
    assert_eq!(v["posti_liberi"], 2);

    manda(&mut ws, lavoro("L1", "S1", 5, "ciao mondo")).await;
    let mut frames = Vec::new();
    loop {
        let f = frame(&mut ws).await.expect("frame");
        let fine = f["tipo"] == "chiuso";
        frames.push(f);
        if fine {
            break;
        }
    }
    assert_eq!(frames[0]["tipo"], "preso");
    assert_eq!(frames[0]["generazione"], 5);
    for (i, f) in frames.iter().enumerate() {
        assert_eq!(f["seq"], i as u64 + 1, "seq contigui: {f}");
        assert_eq!(f["lavoro_id"], "L1");
    }
    let tipi: Vec<&str> = frames
        .iter()
        .filter_map(|f| f["evento"]["tipo"].as_str())
        .collect();
    assert!(
        tipi.contains(&"testo_delta") && tipi.contains(&"finito"),
        "{tipi:?}"
    );
    assert_eq!(frames.last().unwrap()["stato"], "riuscito");
    let ultimo = frames.len();
    let giornale = b.cartella.join("giornale/L1.jsonl");
    assert!(giornale.exists());
    manda(
        &mut ws,
        json!({"tipo": "ok", "lavoro_id": "L1", "fino_a_seq": ultimo}),
    )
    .await;

    // Generazione più vecchia per la stessa sessione: rifiutato.
    manda(&mut ws, lavoro("L0", "S1", 4, "vecchio")).await;
    let f = frame(&mut ws).await.unwrap();
    assert_eq!(
        (f["tipo"].as_str(), f["stato"].as_str()),
        (Some("chiuso"), Some("rifiutato"))
    );
    assert_eq!(f["lavoro_id"], "L0");
    assert!(!giornale.exists(), "giornale confermato e cancellato");
    // Nessun segreto nel giornale rimasto.
    let t = std::fs::read_to_string(b.cartella.join("giornale/L0.jsonl")).unwrap();
    assert!(!t.contains("non-nei-log"));
    // La cartella del turno è stata cancellata.
    assert!(!b.cartella.join("turni/L1").exists());
    let _ = std::fs::remove_dir_all(&b.cartella);
}

#[tokio::test]
async fn ripresa_dopo_il_taglio() {
    let mut b = banco().await;
    let mut ws = b.accettati.recv().await.unwrap();
    manda(&mut ws, lavoro("L2", "S2", 1, "un testo abbastanza lungo")).await;
    let f = frame(&mut ws).await.unwrap();
    assert_eq!(f["tipo"], "preso");
    // Taglio brusco: il turno continua sul Mac.
    drop(ws);
    let mut ws = tokio::time::timeout(Duration::from_secs(15), b.accettati.recv())
        .await
        .unwrap()
        .unwrap();
    let mut visti = Vec::new();
    loop {
        let f = frame(&mut ws).await.expect("frame dopo la ripresa");
        let fine = f["tipo"] == "chiuso";
        visti.push(f);
        if fine {
            break;
        }
    }
    // Rimanda tutto quello che non è confermato: dal preso alla chiusura, senza buchi.
    let seq: Vec<u64> = visti.iter().map(|f| f["seq"].as_u64().unwrap()).collect();
    assert_eq!(seq, (1..=seq.len() as u64).collect::<Vec<_>>(), "{visti:?}");
    assert_eq!(visti[0]["tipo"], "preso");
    assert_eq!(visti.last().unwrap()["stato"], "riuscito");
    manda(
        &mut ws,
        json!({"tipo": "ok", "lavoro_id": "L2", "fino_a_seq": seq.len()}),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!b.cartella.join("giornale/L2.jsonl").exists());
    let _ = std::fs::remove_dir_all(&b.cartella);
}

#[tokio::test]
async fn interrompi_e_provider_assente() {
    let mut b = banco().await;
    let mut ws = b.accettati.recv().await.unwrap();
    let mut l = lavoro("L3", "S3", 1, "x");
    l["richiesta"]["provider_id"] = json!("gemini_inesistente");
    manda(&mut ws, l).await;
    let mut ultimo = Value::Null;
    while ultimo["tipo"] != "chiuso" {
        ultimo = frame(&mut ws).await.unwrap();
    }
    assert_eq!(ultimo["stato"], "fallito");
    assert!(ultimo["motivo"].as_str().unwrap().contains("adattatore"));
    let _ = std::fs::remove_dir_all(&b.cartella);
}

#[tokio::test]
async fn allegati_scritti_e_cancellati_a_fine_turno() {
    let mut b = banco().await;
    let mut ws = b.accettati.recv().await.unwrap();
    let mut l = lavoro("LA", "SA", 1, "guarda l'allegato");
    l["richiesta"]["allegati"] = json!([{
        "nome": "nota.txt", "tipo_mime": "text/plain", "dimensione": 4,
        "contenuto_base64": "Y2lhbw=="
    }]);
    manda(&mut ws, l).await;
    let mut visto_durante = false;
    loop {
        let f = frame(&mut ws).await.expect("frame");
        if f["tipo"] == "preso" || f["tipo"] == "evento" {
            visto_durante |= b.cartella.join("turni/LA/allegati/nota.txt").exists();
        }
        if f["tipo"] == "chiuso" {
            assert_eq!(f["stato"], "riuscito", "{f}");
            break;
        }
    }
    assert!(visto_durante, "l'allegato c'era durante il turno");
    assert!(
        !b.cartella.join("turni/LA").exists(),
        "cancellato a fine turno"
    );
    let _ = std::fs::remove_dir_all(&b.cartella);
}
