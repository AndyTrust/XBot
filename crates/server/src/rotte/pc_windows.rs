//! Un PC Windows dentro XBot (terminale e schermo), con l'accesso di XBot.
//! I pezzi stanno già sul server: ttyd (base `/term/pc`, tmux «pc») e noVNC con websockify
//! (verso il VNC del PC attraverso un ponte). XBot li raggiunge all'indirizzo di
//! `XBOT_PC_WINDOWS` (per esempio il gateway della rete docker); senza quella variabile la voce
//! non c'è.
//! Il ponte, Caddy e i servizi del PC non vengono toccati: XBot fa solo da proxy in lettura.

use axum::body::{Body, Bytes};
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{OriginalUri, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use super::computer::ponte_ws;
use crate::stato::Stato;

const BASE_TERMINALE: &str = "/api/v1/pc-windows/terminale";
const BASE_SCHERMO: &str = "/api/v1/pc-windows/schermo";
/// Il `--base-path` con cui gira ttyd sulla VPS.
const BASE_TTYD: &str = "/term/pc";

fn porta(nome: &str, predefinita: u16) -> u16 {
    std::env::var(nome)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(predefinita)
}

/// L'indirizzo dei servizi del PC, se la voce è accesa.
fn host() -> Option<String> {
    std::env::var("XBOT_PC_WINDOWS")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
}

/// `GET /pc-windows`: se la voce c'è e dove aprire terminale e schermo.
pub async fn leggi() -> axum::Json<Value> {
    if host().is_none() {
        return axum::Json(json!({ "attivo": false }));
    }
    axum::Json(json!({
        "attivo": true, "nome": "Computer Windows",
        "terminale": format!("{BASE_TERMINALE}/"),
        "schermo": format!("{BASE_SCHERMO}/vnc.html?autoconnect=1&resize=scale&reconnect=1&path={}", &format!("{BASE_SCHERMO}/websockify")[1..]),
    }))
}

/// Il percorso da chiedere al servizio vero: toglie la base di XBot e rimette quella di ttyd.
fn percorso_interno(uri: &axum::http::Uri, base: &str, nuova_base: &str) -> Option<String> {
    let resto = uri.path().strip_prefix(base)?;
    if !(resto.is_empty() || resto.starts_with('/')) || resto.contains("..") {
        return None;
    }
    let q = uri.query().map(|q| format!("?{q}")).unwrap_or_default();
    Some(format!("{nuova_base}{resto}{q}"))
}

/// Dove va una richiesta: porta del servizio, base di XBot, base del servizio, sottoprotocollo WebSocket.
struct Bersaglio {
    porta: u16,
    base: &'static str,
    nuova_base: &'static str,
    protocollo: &'static str,
}

async fn inoltra(
    s: Stato,
    b: Bersaglio,
    uri: axum::http::Uri,
    metodo: Method,
    intestazioni: HeaderMap,
    ws: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
    corpo: Bytes,
) -> Response {
    let Some(h) = host() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let (porta_servizio, protocollo) = (b.porta, b.protocollo);
    let Some(percorso) = percorso_interno(&uri, b.base, b.nuova_base) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if let Ok(ws) = ws {
        let url = format!("ws://{h}:{porta_servizio}{percorso}");
        return ws
            .protocols([protocollo])
            .on_upgrade(move |socket| async move { ponte_ws(socket, &url, protocollo).await })
            .into_response();
    }
    let url = format!("http://{h}:{porta_servizio}{percorso}");
    let mut r = s.motore.computer().http().request(metodo, &url);
    if let Some(t) = intestazioni.get(header::CONTENT_TYPE) {
        r = r.header(header::CONTENT_TYPE, t);
    }
    if !corpo.is_empty() {
        r = r.body(corpo);
    }
    match r.send().await {
        Ok(risp) => {
            let mut b = Response::builder().status(risp.status().as_u16());
            for h in [header::CONTENT_TYPE, header::CACHE_CONTROL] {
                if let Some(v) = risp.headers().get(&h) {
                    b = b.header(h, v.as_bytes());
                }
            }
            match risp.bytes().await {
                Ok(dati) => b
                    .body(Body::from(dati))
                    .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response()),
                Err(_) => StatusCode::BAD_GATEWAY.into_response(),
            }
        }
        Err(e) => {
            tracing::warn!(errore = %e, "servizio del PC Windows non raggiungibile");
            StatusCode::BAD_GATEWAY.into_response()
        }
    }
}

pub async fn terminale(
    State(s): State<Stato>,
    OriginalUri(uri): OriginalUri,
    metodo: Method,
    intestazioni: HeaderMap,
    ws: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
    corpo: Bytes,
) -> Response {
    let p = porta("XBOT_PC_TERMINALE_PORTA", 7682);
    inoltra(
        s,
        Bersaglio {
            porta: p,
            base: BASE_TERMINALE,
            nuova_base: BASE_TTYD,
            protocollo: "tty",
        },
        uri,
        metodo,
        intestazioni,
        ws,
        corpo,
    )
    .await
}

pub async fn schermo(
    State(s): State<Stato>,
    OriginalUri(uri): OriginalUri,
    metodo: Method,
    intestazioni: HeaderMap,
    ws: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
    corpo: Bytes,
) -> Response {
    let p = porta("XBOT_PC_SCHERMO_PORTA", 6081);
    inoltra(
        s,
        Bersaglio {
            porta: p,
            base: BASE_SCHERMO,
            nuova_base: "",
            protocollo: "binary",
        },
        uri,
        metodo,
        intestazioni,
        ws,
        corpo,
    )
    .await
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn percorsi() {
        let u: axum::http::Uri = "/api/v1/pc-windows/terminale/ws?x=1".parse().unwrap();
        assert_eq!(
            percorso_interno(&u, BASE_TERMINALE, BASE_TTYD).unwrap(),
            "/term/pc/ws?x=1"
        );
        let u: axum::http::Uri = "/api/v1/pc-windows/terminale".parse().unwrap();
        assert_eq!(
            percorso_interno(&u, BASE_TERMINALE, BASE_TTYD).unwrap(),
            "/term/pc"
        );
        let u: axum::http::Uri = "/api/v1/pc-windows/terminalexx".parse().unwrap();
        assert!(percorso_interno(&u, BASE_TERMINALE, BASE_TTYD).is_none());
        let u: axum::http::Uri = "/api/v1/pc-windows/schermo/vnc.html".parse().unwrap();
        assert_eq!(percorso_interno(&u, BASE_SCHERMO, "").unwrap(), "/vnc.html");
    }
}
