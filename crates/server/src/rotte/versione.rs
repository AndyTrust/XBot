//! `GET /versione` e `GET /eventi`.

use axum::extract::State;
use serde::Deserialize;
use serde_json::{Value, json};

use super::json_db;
use crate::errore_http::{Domanda, RisHttp};
use crate::stato::Stato;

/// Il commit e il momento della compilazione (da `build.rs`).
pub const COMMIT: &str = env!("XBOT_COMMIT");
pub const COMPILATO_MS: &str = env!("XBOT_COMPILATO_MS");

pub async fn versione(State(s): State<Stato>) -> RisHttp<Value> {
    let (avviato, macchina) = (s.servizi.avviato_ms, s.servizi.macchina.clone());
    json_db(&s, move |c| {
        Ok(json!({
            "versione": xbot_core::VERSIONE,
            "commit": COMMIT,
            "compilato_ms": COMPILATO_MS.parse::<i64>().ok(),
            "schema": xbot_db::versione_schema(c)?,
            "avviato_ms": avviato,
            "macchina": { "id": macchina.id, "nome": macchina.nome, "genere": macchina.genere },
        }))
    })
    .await
}

#[derive(Deserialize)]
pub struct Limite {
    pub limite: Option<i64>,
}

pub async fn eventi(State(s): State<Stato>, Domanda(l): Domanda<Limite>) -> RisHttp<Value> {
    json_db(&s, move |c| {
        Ok(json!({ "eventi": xbot_db::eventi::ultimi(c, l.limite.unwrap_or(100))? }))
    })
    .await
}
