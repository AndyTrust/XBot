//! `enroll` contro un server HTTP finto: corpo della richiesta e credenziale 0600.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::os::unix::fs::PermissionsExt;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use xbot_worker::iscrizione::{Iscrizione, iscrivi};

async fn server_finto(
    stato: &'static str,
    risposta: &'static str,
) -> (String, tokio::task::JoinHandle<(String, Value)>) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let h = tokio::spawn(async move {
        let (mut s, _) = l.accept().await.unwrap();
        let mut buf = Vec::new();
        let mut pezzo = [0u8; 4096];
        loop {
            let n = s.read(&mut pezzo).await.unwrap();
            buf.extend_from_slice(&pezzo[..n]);
            let t = String::from_utf8_lossy(&buf).to_string();
            if let Some(i) = t.find("\r\n\r\n") {
                let lung: usize = t[..i]
                    .lines()
                    .find_map(|r| {
                        r.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap())
                    })
                    .unwrap_or(0);
                if buf.len() >= i + 4 + lung {
                    let prima = t.lines().next().unwrap().to_string();
                    let corpo: Value = serde_json::from_slice(&buf[i + 4..i + 4 + lung]).unwrap();
                    let r = format!(
                        "HTTP/1.1 {stato}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{risposta}",
                        risposta.len()
                    );
                    s.write_all(r.as_bytes()).await.unwrap();
                    return (prima, corpo);
                }
            }
        }
    });
    (url, h)
}

#[tokio::test]
async fn enroll_salva_la_credenziale() {
    let (url, h) = server_finto(
        "200 OK",
        r#"{"macchina_id":"M9","credenziale":"c-segreta"}"#,
    )
    .await;
    let d = std::env::temp_dir().join(format!("plw-e-{}", xbot_core::nuovo_id()));
    let c = iscrivi(
        &d,
        Iscrizione {
            url: &format!("{url}/"),
            biglietto: "BIG",
            nome: "mac-prova",
            posti: 3,
            provider: Vec::new(),
        },
    )
    .await
    .unwrap();
    let (prima, corpo) = h.await.unwrap();
    assert_eq!(prima, "POST /api/v1/macchine/collega HTTP/1.1");
    assert_eq!(corpo["biglietto"], "BIG");
    assert_eq!(corpo["genere"], "mac");
    assert_eq!(corpo["posti"], 3);
    assert_eq!(
        corpo["capacita"],
        serde_json::json!(["turni", "chrome", "file"])
    );
    assert!(corpo["chiave_pubblica"].as_str().unwrap().len() > 100);
    assert!(corpo["provider"].is_array());
    assert_eq!(c.macchina_id, "M9");
    let f = d.join("credenziale.json");
    assert_eq!(
        std::fs::metadata(&f).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    assert_eq!(v["credenziale"], "c-segreta");
    assert_eq!(v["url"], url);
    assert!(!format!("{c:?}").contains("c-segreta"));
    std::fs::remove_dir_all(&d).unwrap();
}

#[tokio::test]
async fn enroll_biglietto_scaduto() {
    let (url, _h) = server_finto("401 Unauthorized", r#"{"errore":"biglietto scaduto"}"#).await;
    let d = std::env::temp_dir().join(format!("plw-e-{}", xbot_core::nuovo_id()));
    let e = iscrivi(
        &d,
        Iscrizione {
            url: &url,
            biglietto: "X",
            nome: "mac",
            posti: 1,
            provider: Vec::new(),
        },
    )
    .await
    .unwrap_err();
    assert!(e.testo.contains("401"), "{e}");
    assert!(!d.join("credenziale.json").exists());
}
