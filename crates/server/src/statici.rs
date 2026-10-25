//! I file dell'interfaccia (`web/`): alla radice (`/`, `/stile.css`, `/app.js`, `/sw.js`,
//! `/manifest.webmanifest`, `/icone/*`) e sotto `/app/*`. Un percorso che non è un file dà
//! `index.html`. Le rotte `/api/*` hanno la precedenza (le monta il chiamante prima di queste).
//! Con `XBOT_NOME` il manifest esce con `name` e `short_name` uguali al nome del bot.

use std::path::{Path, PathBuf};

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{Next, from_fn};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tower_http::services::{ServeDir, ServeFile};

/// Il service worker può controllare tutta l'origine anche se servito da `/app/sw.js`. Tutti i file
/// dell'interfaccia hanno `Cache-Control: no-cache` (2026-10-25): il browser ricontrolla con l'ETag a
/// ogni apertura, così dopo un rilascio nessun modulo vecchio resta in cache (un `import` di una
/// funzione nuova da un modulo vecchio lascia la pagina bianca).
async fn service_worker(req: Request, next: Next) -> Response {
    let sw = matches!(req.uri().path(), "/sw.js" | "/app/sw.js");
    let mut r = next.run(req).await;
    if sw {
        r.headers_mut()
            .insert("service-worker-allowed", HeaderValue::from_static("/"));
    }
    if !r.headers().contains_key(header::CACHE_CONTROL) {
        r.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    }
    r
}

/// Il manifest con `name` e `short_name` dal nome configurato.
fn manifest_con_nome(testo: &str, nome: &str) -> Option<String> {
    let mut v: serde_json::Value = serde_json::from_str(testo).ok()?;
    let o = v.as_object_mut()?;
    o.insert("name".into(), nome.into());
    o.insert("short_name".into(), nome.into());
    serde_json::to_string_pretty(&v).ok()
}

async fn manifest(file: PathBuf) -> Response {
    let nome = xbot_core::nome_bot();
    match tokio::fs::read_to_string(&file).await {
        Ok(t) => match manifest_con_nome(&t, &nome) {
            Some(m) => ([(header::CONTENT_TYPE, "application/manifest+json")], m).into_response(),
            None => ([(header::CONTENT_TYPE, "application/manifest+json")], t).into_response(),
        },
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Il router dei file statici, da unire dopo le rotte dell'API.
pub fn router(web: &Path) -> Router {
    let indice = web.join("index.html");
    let cartella = || ServeDir::new(web).fallback(ServeFile::new(&indice));
    let mut r = Router::new();
    if std::env::var("XBOT_NOME").is_ok_and(|n| !n.trim().is_empty()) {
        for percorso in ["/manifest.webmanifest", "/app/manifest.webmanifest"] {
            let file = web.join("manifest.webmanifest");
            r = r.route(percorso, get(move || manifest(file.clone())));
        }
    }
    r.route_service("/", ServeFile::new(&indice))
        .nest_service("/app", cartella())
        .fallback_service(cartella())
        .layer(from_fn(service_worker))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn manifest_col_nome() {
        let m = manifest_con_nome(
            r#"{"name":"XBot","short_name":"XBot","lang":"it"}"#,
            "Atena",
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&m).unwrap();
        assert_eq!(
            (v["name"].as_str(), v["short_name"].as_str()),
            (Some("Atena"), Some("Atena"))
        );
        assert_eq!(v["lang"], "it");
        assert!(manifest_con_nome("non json", "Atena").is_none());
    }
}
