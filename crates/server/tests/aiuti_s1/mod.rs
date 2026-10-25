//! Aiuti delle prove di S1 (Fase 2.D): server con la cassaforte aperta, chiamate multipart.
#![allow(dead_code)]

use crate::comune::*;
use xbot_server::servizi::OpzioniServizi;

pub const FRASE: &str = "frase-della-cassaforte-di-prova";

/// Come `comune::prova`, con la cassaforte aperta (o chiusa con `None`).
pub async fn prova_s1(chiave: Option<&str>) -> Prova {
    let db = Db::apri(":memory:").unwrap();
    let segreto = base64::engine::general_purpose::STANDARD.encode([9u8; 32]);
    let stato = Stato::prepara_servizi(
        db.clone(),
        vec![Arc::new(Finto::con_ritardo(Duration::from_millis(5)))],
        Some(SegretoTesto::nuovo(segreto)),
        false,
        Vec::new(),
        OpzioniServizi::per_prove(chiave),
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
    Prova {
        app: app(stato.clone(), Path::new("web-che-non-esiste")),
        stato,
        db,
        url_totp,
    }
}

/// Una chiamata multipart con un solo file.
pub async fn multipart(
    p: &Prova,
    uri: &str,
    cookie: &str,
    campo: &str,
    nome: &str,
    tipo: &str,
    byte: &[u8],
) -> (StatusCode, Value) {
    let confine = "----confine-xbot";
    let mut corpo = format!(
        "--{confine}\r\nContent-Disposition: form-data; name=\"{campo}\"; filename=\"{nome}\"\r\nContent-Type: {tipo}\r\n\r\n"
    )
    .into_bytes();
    corpo.extend_from_slice(byte);
    corpo.extend_from_slice(format!("\r\n--{confine}--\r\n").as_bytes());
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::COOKIE, cookie)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={confine}"),
        )
        .body(Body::from(corpo))
        .unwrap();
    let r = p.app.clone().oneshot(req).await.unwrap();
    let stato = r.status();
    let b = r.into_body().collect().await.unwrap().to_bytes();
    (stato, serde_json::from_slice(&b).unwrap_or(Value::Null))
}

/// GET grezzo: stato, intestazioni e byte.
pub async fn grezzo(
    p: &Prova,
    uri: &str,
    cookie: &str,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let req = Request::builder()
        .uri(uri)
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .unwrap();
    let r = p.app.clone().oneshot(req).await.unwrap();
    let (stato, h) = (r.status(), r.headers().clone());
    (
        stato,
        h,
        r.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
}
