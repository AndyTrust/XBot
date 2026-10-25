//! `POST /sessioni/{id}/invii` e `POST /turni/{id}/interrompi`.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::Value;
use xbot_db::turni::Turno;

use crate::errore_http::{Corpo, RisHttp};
use crate::motore::{DatiInvio, EsitoInvio};
use crate::stato::Stato;

#[derive(Deserialize)]
pub struct CorpoInvio {
    pub id: String,
    pub testo: String,
    #[serde(default)]
    pub allegati: Option<Value>,
    pub risposta_a: Option<String>,
    pub provider: Option<String>,
    pub macchina: Option<String>,
    pub origine: Option<String>,
    pub composto_ms: Option<i64>,
}

pub async fn invia(
    State(s): State<Stato>,
    Path(sessione): Path<String>,
    Corpo(b): Corpo<CorpoInvio>,
) -> RisHttp<EsitoInvio> {
    let d = DatiInvio {
        id: b.id,
        testo: b.testo,
        allegati: b.allegati.unwrap_or_else(|| Value::Array(vec![])),
        risposta_a: b.risposta_a,
        provider: b.provider,
        macchina: b.macchina,
        origine: b.origine.unwrap_or_else(|| "pwa".into()),
        composto_ms: b.composto_ms,
    };
    Ok(Json(s.motore.invia(&sessione, d).await?))
}

pub async fn interrompi(State(s): State<Stato>, Path(turno): Path<String>) -> RisHttp<Turno> {
    Ok(Json(s.motore.interrompi(&turno).await?))
}
