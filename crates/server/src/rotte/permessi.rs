//! `GET /permessi`, `DELETE /permessi/{id}`, `GET`/`PUT /regole-revisione`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use xbot_db::permessi::{self as repo, NuovaRegola};

use super::json_db;
use crate::errore_http::{Corpo, ErroreHttp, RisHttp};
use crate::stato::Stato;

pub async fn elenca(State(s): State<Stato>) -> RisHttp<Value> {
    json_db(&s, |c| Ok(json!({ "permessi": repo::elenca(c)? }))).await
}

pub async fn ritira(
    State(s): State<Stato>,
    Path(id): Path<String>,
) -> Result<StatusCode, ErroreHttp> {
    s.db.esegui(move |c| repo::ritira(c, &id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn regole(State(s): State<Stato>) -> RisHttp<Value> {
    json_db(&s, |c| Ok(json!({ "regole": repo::regole(c)? }))).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpoRegole {
    pub regole: Vec<NuovaRegola>,
}

/// Sostituisce tutte le regole (al massimo 20, ciascuna al massimo 1000 caratteri).
pub async fn scrivi_regole(State(s): State<Stato>, Corpo(b): Corpo<CorpoRegole>) -> RisHttp<Value> {
    json_db(&s, move |c| {
        Ok(json!({ "regole": repo::scrivi_regole(c, &b.regole)? }))
    })
    .await
}
