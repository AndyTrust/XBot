//! Sessioni figlie e lavagna comune: le rotte dell'utente (e dell'interfaccia).
//! `GET|POST /sessioni/{id}/figlie`, `GET /lavagna`, `POST /lavagna/{id}/chiudi`.

use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use xbot_db::{figlie, lavagna};

use super::json_db;
use crate::errore_http::{Corpo, RisHttp};
use crate::mcp_interno::figlie as srv;
use crate::stato::Stato;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NuovaFiglia {
    pub compito: String,
    #[serde(default)]
    pub titolo: String,
    /// `@nome` dell'assistente a cui affidarla (se manca, lo stesso del padre).
    #[serde(default)]
    pub menzione: String,
}

/// `GET /sessioni/{id}/figlie` → `{figlie: [{id, titolo, figlia_stato, figlia_riassunto, …}]}`.
pub async fn elenco(State(s): State<Stato>, Path(sid): Path<String>) -> RisHttp<Value> {
    json_db(&s, move |c| {
        Ok(json!({ "figlie": figlie::elenco(c, &sid)? }))
    })
    .await
}

/// `POST /sessioni/{id}/figlie {compito, titolo?, menzione?}` → `{figlia_id, titolo}`.
pub async fn apri(
    State(s): State<Stato>,
    Path(sid): Path<String>,
    Corpo(n): Corpo<NuovaFiglia>,
) -> RisHttp<Value> {
    srv::apri_da(
        &s,
        &sid,
        n.menzione.trim(),
        n.compito.trim(),
        n.titolo.trim(),
    )
    .await
    .map(Json)
    .map_err(|e| xbot_core::Errore::non_valida(e).into())
}

/// `GET /lavagna?progetto=` → `{voci: […]}`: le voci vive.
pub async fn leggi(
    State(s): State<Stato>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> RisHttp<Value> {
    let progetto = q.get("progetto").filter(|p| !p.is_empty()).cloned();
    json_db(&s, move |c| {
        Ok(json!({ "voci": lavagna::attive(c, None, progetto.as_deref())? }))
    })
    .await
}

/// `POST /lavagna/{id}/chiudi` → `{chiusa}`: l'utente chiude una voce morta o dimenticata.
pub async fn chiudi(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    json_db(&s, move |c| {
        Ok(json!({ "chiusa": lavagna::chiudi_id(c, &id, "chiusa dall'utente")? }))
    })
    .await
}

use axum::Json;
