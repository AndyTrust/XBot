//! `/assistenti` e `/sezioni`.

use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Map, Value};
use xbot_core::nuovo_id;
use xbot_db::assistenti::{self as repo, NuovoAssistente};
use xbot_db::sezioni::{self, ModificaSezione};

use super::json_db;
use crate::errore_http::{Corpo, Domanda, RisHttp};
use crate::stato::Stato;

#[derive(Deserialize)]
pub struct FiltroAssistenti {
    #[serde(default)]
    pub archiviati: bool,
}

pub async fn elenca(
    State(s): State<Stato>,
    Domanda(f): Domanda<FiltroAssistenti>,
) -> RisHttp<Vec<Value>> {
    json_db(&s, move |c| repo::elenca(c, f.archiviati)).await
}

pub async fn crea(State(s): State<Stato>, Corpo(n): Corpo<NuovoAssistente>) -> RisHttp<Value> {
    json_db(&s, move |c| repo::crea(c, &n)).await
}

pub async fn leggi(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    json_db(&s, move |c| repo::leggi(c, &id)).await
}

pub async fn aggiorna(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Corpo(m): Corpo<Map<String, Value>>,
) -> RisHttp<Value> {
    json_db(&s, move |c| repo::aggiorna(c, &id, &m)).await
}

/// DELETE = archivia.
pub async fn archivia(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    json_db(&s, move |c| repo::archivia(c, &id)).await
}

pub async fn sezioni(State(s): State<Stato>) -> RisHttp<Vec<Value>> {
    json_db(&s, sezioni::elenca).await
}

#[derive(Deserialize)]
pub struct NuovaSezione {
    pub id: Option<String>,
    pub nome: String,
    #[serde(default)]
    pub ordine: i64,
}

pub async fn crea_sezione(State(s): State<Stato>, Corpo(n): Corpo<NuovaSezione>) -> RisHttp<Value> {
    let id = n.id.filter(|i| !i.is_empty()).unwrap_or_else(nuovo_id);
    json_db(&s, move |c| sezioni::crea(c, &id, &n.nome, n.ordine)).await
}

/// PATCH con un elenco `[{id, nome?, ordine?, chiusa?}]`: riordino in un colpo.
pub async fn aggiorna_sezioni(
    State(s): State<Stato>,
    Corpo(m): Corpo<Vec<ModificaSezione>>,
) -> RisHttp<Vec<Value>> {
    json_db(&s, move |c| sezioni::aggiorna_molte(c, &m)).await
}
