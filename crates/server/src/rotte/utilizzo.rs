//! `GET /utilizzo?da_ms=&a_ms=&assistente_id=&provider_id=&modello_id=`: token e costo per provider, assistente, modello e giorno.

use axum::extract::State;
use serde::Deserialize;
use serde_json::Value;
use xbot_core::ora_ms;

use super::json_db;
use crate::errore_http::{Domanda, RisHttp};
use crate::stato::Stato;

#[derive(Deserialize)]
pub struct Periodo {
    pub da_ms: Option<i64>,
    pub a_ms: Option<i64>,
    pub assistente_id: Option<String>,
    pub provider_id: Option<String>,
    pub modello_id: Option<String>,
}

/// Senza parametri: gli ultimi 30 giorni.
pub async fn utilizzo(State(s): State<Stato>, Domanda(p): Domanda<Periodo>) -> RisHttp<Value> {
    let a = p.a_ms.unwrap_or_else(|| ora_ms() + 1);
    let da = p.da_ms.unwrap_or(a - 30 * 86_400_000);
    let f = xbot_db::utilizzo::Filtri {
        assistente_id: p.assistente_id,
        provider_id: p.provider_id,
        modello_id: p.modello_id,
    };
    json_db(&s, move |c| xbot_db::utilizzo::riepilogo(c, da, a, &f)).await
}
