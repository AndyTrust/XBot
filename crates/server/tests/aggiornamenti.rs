//! Prove degli aggiornamenti (CONTRATTO-GITHUB.md, sezione E): un finto GitHub su 127.0.0.1,
//! cache di 6 ore, `?forza=1`, modo `no`. Una sola prova in questo file: la cache è comune.

mod comune;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use comune::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Un finto `GET /repos/<repo>/releases/latest`: risponde con `tag` e conta le richieste.
async fn finto_github(tag: &'static str) -> (String, Arc<AtomicUsize>) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let indirizzo = format!("http://{}", l.local_addr().unwrap());
    let conta = Arc::new(AtomicUsize::new(0));
    let c = conta.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else {
                break;
            };
            let c = c.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let richiesta = String::from_utf8_lossy(&buf[..n]).to_string();
                let giusta = richiesta.starts_with("GET /repos/prova/xbot/releases/latest ")
                    && richiesta.to_lowercase().contains("user-agent: xbot/")
                    && richiesta.contains("application/vnd.github+json");
                c.fetch_add(1, Ordering::SeqCst);
                let (stato, corpo) = if giusta {
                    (
                        "200 OK",
                        format!(
                            r#"{{"tag_name":"{tag}","html_url":"https://esempio.it/note/{tag}"}}"#
                        ),
                    )
                } else {
                    ("404 Not Found", r#"{"message":"Not Found"}"#.to_string())
                };
                let r = format!(
                    "HTTP/1.1 {stato}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{corpo}",
                    corpo.len()
                );
                let _ = s.write_all(r.as_bytes()).await;
            });
        }
    });
    (indirizzo, conta)
}

#[tokio::test]
async fn controllo_cache_forza_e_modo_no() {
    let (api, conta) = finto_github("v2099.1.1").await;
    let imposta = |api: &str| {
        xbot_server::aggiornamenti::imposta_per_prove(Some(
            xbot_server::aggiornamenti::Impostazioni {
                repo: "prova/xbot".into(),
                api: api.into(),
                modo: "no".into(),
            },
        ))
    };
    imposta(&api);
    let p = prova(5).await;
    let (s, v, _) = p.chiama("GET", "/api/v1/salute", None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["versione_installata"], xbot_core::VERSIONE);

    let (s, _, _) = p.chiama("GET", "/api/v1/aggiornamenti", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let c = p.entra().await;
    let (s, v, _) = p
        .chiama("GET", "/api/v1/aggiornamenti", None, Some(&c))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["versione_installata"], xbot_core::VERSIONE);
    assert_eq!(v["versione_remota"], "v2099.1.1");
    assert_eq!(v["disponibile"], true);
    assert_eq!(v["modo"], "no");
    assert_eq!(v["note_url"], "https://esempio.it/note/v2099.1.1");
    assert!(v["controllato_ms"].as_i64().unwrap() > 0);
    assert_eq!(conta.load(Ordering::SeqCst), 1);

    // Dalla cache: GitHub non si richiama; con ?forza=1 sì.
    let (_, v2, _) = p
        .chiama("GET", "/api/v1/aggiornamenti", None, Some(&c))
        .await;
    assert_eq!(v2["controllato_ms"], v["controllato_ms"]);
    assert_eq!(conta.load(Ordering::SeqCst), 1);
    p.chiama("GET", "/api/v1/aggiornamenti?forza=1", None, Some(&c))
        .await;
    assert_eq!(conta.load(Ordering::SeqCst), 2);

    // Modo `no`: l'avvio è vietato.
    let (s, v, _) = p
        .chiama("POST", "/api/v1/aggiornamenti/avvia", None, Some(&c))
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(v["errore"]["codice"], "aggiornamenti_disattivati");

    // GitHub che non risponde: disponibile null, errore, mai 500.
    imposta("http://127.0.0.1:9");
    let (s, v, _) = p
        .chiama("GET", "/api/v1/aggiornamenti?forza=1", None, Some(&c))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["disponibile"], serde_json::Value::Null);
    assert!(v["errore"].as_str().unwrap().contains("GitHub"), "{v}");
}
