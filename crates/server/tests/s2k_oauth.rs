//! Prove della 2.K, parte OAuth: MCP generico con 401 o con i soli metadati → «serve il
//! collegamento dell'account». A mano (rete vera o Docker): Slack con un client finto, Google
//! ospitato dietro `caddy-google-finto.py`.

mod comune;
mod s2_comune;
mod s2k_comune;

use comune::*;
use s2k_comune::*;

/// Porta di `oauth-finto.py` (la 2.I usa la 8831).
const PORTA_OAUTH: u16 = 8832;

/// Avvia `oauth-finto.py` sulla sua porta e aspetta che risponda.
async fn oauth_finto() -> tokio::process::Child {
    let script = format!("{}/tests/oauth-finto.py", env!("CARGO_MANIFEST_DIR"));
    let figlio = tokio::process::Command::new("python3")
        .arg(script)
        .arg(PORTA_OAUTH.to_string())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let stato = format!("http://127.0.0.1:{PORTA_OAUTH}/stato");
    for _ in 0..100 {
        if reqwest::get(&stato).await.is_ok() {
            return figlio;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("oauth-finto.py non risponde");
}

#[tokio::test]
async fn mcp_generico_con_oauth_chiede_l_account() {
    let _f = oauth_finto().await;
    let (p, c, _) = banco(None).await;
    // 401 con WWW-Authenticate.
    let url = format!("http://127.0.0.1:{PORTA_OAUTH}/mcp");
    let id = crea(
        &p,
        &c,
        json!({ "nome": "Generico", "trasporto": "http", "url": url }),
    )
    .await;
    let (_, v) = chiama(&p, &c, "GET", &format!("/api/v1/collegamenti/{id}"), None).await;
    assert_eq!(v["richiede_oauth"], false);
    let e = prova_coll(&p, &c, &id).await;
    assert_eq!(e["ok"], false);
    assert_eq!(e["errore"], "serve il collegamento dell'account");
    let (_, v) = chiama(&p, &c, "GET", &format!("/api/v1/collegamenti/{id}"), None).await;
    assert_eq!(v["richiede_oauth"], true);
    assert_eq!(v["ultimo_errore"], "serve il collegamento dell'account");
    // Nessuna sfida (403) ma i metadati della risorsa ci sono: stessa strada.
    let url = format!("http://127.0.0.1:{PORTA_OAUTH}/muto");
    let id = crea(
        &p,
        &c,
        json!({ "nome": "Solo metadati", "trasporto": "http", "url": url }),
    )
    .await;
    let e = prova_coll(&p, &c, &id).await;
    assert_eq!(e["errore"], "serve il collegamento dell'account", "{e}");
    let (_, v) = chiama(&p, &c, "GET", &format!("/api/v1/collegamenti/{id}"), None).await;
    assert_eq!(v["richiede_oauth"], true);
    // Un server qualunque che non risponde e non dichiara OAuth resta un errore normale.
    let id = crea(
        &p,
        &c,
        json!({ "nome": "Spento", "trasporto": "http", "url": "http://127.0.0.1:9/mcp" }),
    )
    .await;
    let e = prova_coll(&p, &c, &id).await;
    assert_ne!(e["errore"], "serve il collegamento dell'account", "{e}");
    let (_, v) = chiama(&p, &c, "GET", &format!("/api/v1/collegamenti/{id}"), None).await;
    assert_eq!(v["richiede_oauth"], false);
}

/// L'url di autorizzazione di `avvia` (corpo vuoto).
async fn avvia(p: &Prova, c: &str, id: &str) -> (StatusCode, Value) {
    chiama(
        p,
        c,
        "POST",
        &format!("/api/v1/collegamenti/{id}/oauth/avvia"),
        None,
    )
    .await
}

/// Solo a mano (rete vera, sola lettura: metadati di Slack, nessun consenso):
/// `cargo test -p xbot-server --test s2k_oauth -- --ignored slack`.
#[tokio::test]
#[ignore]
async fn slack_con_client_finto_a_mano() {
    let (p, c, _) = banco(Some("https://xbot.esempio.it")).await;
    let id = crea(
        &p,
        &c,
        json!({ "nome": "Slack", "trasporto": "http", "url": "https://mcp.slack.com/mcp" }),
    )
    .await;
    // Senza client: 502 come prima.
    let (s, _) = avvia(&p, &c, &id).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    let corpo = json!({ "client_id": "1234.5678-finto", "client_secret": "segreto-finto" });
    let (s, v) = chiama(
        &p,
        &c,
        "PUT",
        &format!("/api/v1/collegamenti/{id}/oauth/client"),
        Some(corpo),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v) = avvia(&p, &c, &id).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let url = reqwest::Url::parse(v["url"].as_str().unwrap()).unwrap();
    println!("url di Slack: {url}");
    assert_eq!(
        url.as_str().split('?').next(),
        Some("https://slack.com/oauth/v2_user/authorize")
    );
    let q: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
    assert_eq!(q["client_id"], "1234.5678-finto");
    assert_eq!(
        q["redirect_uri"],
        "https://xbot.esempio.it/api/v1/oauth/ritorno"
    );
    assert_eq!(q["code_challenge_method"], "S256");
    assert!(!q["code_challenge"].is_empty() && !q["state"].is_empty());
    assert!(q["scope"].contains("channels:history"), "{q:?}");
    assert!(!url.as_str().contains("segreto-finto"));
}

/// Solo a mano: il server Google in Docker su 127.0.0.1:18851 dietro `caddy-google-finto.py` (porta
/// 8860, le regole del blocco Caddy di `ops/google-mcp`), con un client Google finto.
#[tokio::test]
#[ignore]
async fn google_ospitato_a_mano() {
    let (p, c, _) = banco(Some("http://127.0.0.1:8860")).await;
    let (_, cat) = chiama(&p, &c, "GET", "/api/v1/collegamenti/catalogo", None).await;
    let g = cat["catalogo"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == "google")
        .unwrap()
        .clone();
    let id = crea(
        &p,
        &c,
        json!({ "nome": g["nome"], "trasporto": "http", "url": g["url"] }),
    )
    .await;
    let e = prova_coll(&p, &c, &id).await;
    println!("prova senza accesso: {e}");
    assert_eq!(e["errore"], "serve il collegamento dell'account");
    let (s, v) = avvia(&p, &c, &id).await;
    println!("avvia: {s} {v}");
    assert_eq!(s, StatusCode::OK, "{v}");
    let url = v["url"].as_str().unwrap();
    assert!(
        url.starts_with("http://127.0.0.1:8860/google-mcp/authorize?"),
        "{url}"
    );
    let (_, v) = chiama(&p, &c, "GET", &format!("/api/v1/collegamenti/{id}"), None).await;
    println!(
        "riga: client_registrato={} oauth={}",
        v["client_registrato"], v["oauth"]
    );
}
