//! Fase 2.H: la scheda Computer. Stato, accendi/spegni/ricrea, lo schermo (WebSocket con
//! protocollo `binary` inoltrato a x11vnc, come fa websockify) e il terminale (HTTP e
//! WebSocket inoltrati a ttyd, che ascolta con `--base-path` uguale al percorso qui).

use axum::body::{Body, Bytes};
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{OriginalUri, Path, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_tungstenite::tungstenite;

use crate::errore_http::RisHttp;
use crate::stato::Stato;

pub async fn leggi(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    Ok(axum::Json(s.motore.computer().info(&id).await?))
}

pub async fn accendi(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    Ok(axum::Json(s.motore.computer().accendi(&id).await?))
}

pub async fn spegni(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    Ok(axum::Json(s.motore.computer().spegni(&id).await?))
}

pub async fn ricrea(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    Ok(axum::Json(s.motore.computer().ricrea(&id).await?))
}

/// 409 `computer_spento` (stesso formato degli errori di XBot; il codice non sta in `Codice`).
fn spento() -> Response {
    (
        StatusCode::CONFLICT,
        axum::Json(serde_json::json!({
            "errore": { "codice": "computer_spento", "testo": "il computer dell'assistente è spento" }
        })),
    )
        .into_response()
}

/// Lo schermo: WebSocket `binary` ↔ TCP di x11vnc.
pub async fn schermo(
    State(s): State<Stato>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(posto) = s.motore.computer().posto(&id) else {
        return spento().into_response();
    };
    let porta = posto.porta_vnc;
    ws.protocols(["binary"])
        .on_upgrade(move |socket| async move {
            match tokio::net::TcpStream::connect(("127.0.0.1", porta as u16)).await {
                Ok(tcp) => ponte_tcp(socket, tcp).await,
                Err(e) => tracing::warn!(errore = %e, "x11vnc non raggiungibile"),
            }
        })
        .into_response()
}

async fn ponte_tcp(socket: WebSocket, tcp: tokio::net::TcpStream) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (mut tcp_rx, mut tcp_tx) = tcp.into_split();
    let a_vnc = async {
        while let Some(Ok(m)) = ws_rx.next().await {
            let dati = match m {
                Message::Binary(b) => b,
                Message::Text(t) => Bytes::from(t.as_str().as_bytes().to_vec()),
                Message::Close(_) => break,
                _ => continue,
            };
            if tcp_tx.write_all(&dati).await.is_err() {
                break;
            }
        }
        let _ = tcp_tx.shutdown().await;
    };
    let dal_vnc = async {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match tcp_rx.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if ws_tx
                        .send(Message::Binary(Bytes::copy_from_slice(&buf[..n])))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
        let _ = ws_tx.send(Message::Close(None)).await;
    };
    tokio::select! {
        () = a_vnc => {}
        () = dal_vnc => {}
    }
}

/// Il terminale: tutto quello che sta sotto `…/computer/terminale` va a ttyd, che ha lo stesso
/// `--base-path`. Una richiesta di upgrade diventa un ponte WebSocket (protocollo `tty`).
pub async fn terminale(
    State(s): State<Stato>,
    Path(parti): Path<Vec<(String, String)>>,
    OriginalUri(uri): OriginalUri,
    metodo: Method,
    intestazioni: HeaderMap,
    ws: Result<WebSocketUpgrade, axum::extract::ws::rejection::WebSocketUpgradeRejection>,
    corpo: Bytes,
) -> Response {
    let Some(id) = parti
        .iter()
        .find(|(k, _)| k == "id")
        .map(|(_, v)| v.clone())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(posto) = s.motore.computer().posto(&id) else {
        return spento().into_response();
    };
    let percorso = uri
        .path_and_query()
        .map_or_else(|| uri.path().to_string(), ToString::to_string);
    // Solo dentro la base del terminale di questo assistente.
    if !uri
        .path()
        .starts_with(&crate::computer::base_terminale(&id))
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    let porta = posto.porta_ttyd;
    if let Ok(ws) = ws {
        let url = format!("ws://127.0.0.1:{porta}{percorso}");
        return ws
            .protocols(["tty"])
            .on_upgrade(move |socket| async move { ponte_ws(socket, &url, "tty").await })
            .into_response();
    }
    let url = format!("http://127.0.0.1:{porta}{percorso}");
    let mut r = s.motore.computer().http().request(metodo, &url);
    if let Some(t) = intestazioni.get(header::CONTENT_TYPE) {
        r = r.header(header::CONTENT_TYPE, t);
    }
    if !corpo.is_empty() {
        r = r.body(corpo);
    }
    match r.send().await {
        Ok(risp) => {
            let stato = risp.status();
            let mut b = Response::builder().status(stato.as_u16());
            for h in [
                header::CONTENT_TYPE,
                header::LOCATION,
                header::CACHE_CONTROL,
            ] {
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
            tracing::warn!(errore = %e, "ttyd non raggiungibile");
            StatusCode::BAD_GATEWAY.into_response()
        }
    }
}

pub(crate) async fn ponte_ws(socket: WebSocket, url: &str, protocollo: &'static str) {
    use tungstenite::client::IntoClientRequest;
    let mut req = match url.into_client_request() {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(errore = %e, "indirizzo di ttyd non valido");
            return;
        }
    };
    req.headers_mut().insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        header::HeaderValue::from_static(protocollo),
    );
    let dentro = match tokio_tungstenite::connect_async(req).await {
        Ok((d, _)) => d,
        Err(e) => {
            tracing::warn!(errore = %e, "WebSocket di ttyd non aperto");
            return;
        }
    };
    let (mut fuori_tx, mut fuori_rx) = socket.split();
    let (mut dentro_tx, mut dentro_rx) = dentro.split();
    let su = async {
        while let Some(Ok(m)) = fuori_rx.next().await {
            let m = match m {
                Message::Binary(b) => tungstenite::Message::Binary(b),
                Message::Text(t) => tungstenite::Message::text(t.as_str()),
                Message::Ping(p) => tungstenite::Message::Ping(p),
                Message::Pong(p) => tungstenite::Message::Pong(p),
                Message::Close(_) => break,
            };
            if dentro_tx.send(m).await.is_err() {
                break;
            }
        }
        let _ = dentro_tx.close().await;
    };
    let giu = async {
        while let Some(Ok(m)) = dentro_rx.next().await {
            let m = match m {
                tungstenite::Message::Binary(b) => Message::Binary(b),
                tungstenite::Message::Text(t) => Message::Text(t.as_str().into()),
                tungstenite::Message::Ping(p) => Message::Ping(p),
                tungstenite::Message::Pong(p) => Message::Pong(p),
                tungstenite::Message::Close(_) => break,
                tungstenite::Message::Frame(_) => continue,
            };
            if fuori_tx.send(m).await.is_err() {
                break;
            }
        }
        let _ = fuori_tx
            .send(Message::Close(Some(CloseFrame {
                code: 1000,
                reason: "".into(),
            })))
            .await;
    };
    tokio::select! {
        () = su => {}
        () = giu => {}
    }
}
