//! `GET /richieste` e `POST /richieste/{id}/risolvi`.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use xbot_core::StatoRichiesta;
use xbot_db::richieste as repo;

use super::json_db;
use crate::errore_http::{Corpo, Domanda, RisHttp};
use crate::richieste::{self, CorpoRisolvi};
use crate::stato::Stato;

#[derive(Deserialize)]
pub struct FiltroRichieste {
    /// `aperta` (predefinito), `approvata`, `negata`, `scaduta`, `ritirata` o `tutte`.
    pub stato: Option<String>,
}

pub async fn elenca(
    State(s): State<Stato>,
    Domanda(f): Domanda<FiltroRichieste>,
) -> RisHttp<Value> {
    let stato = match f.stato.as_deref() {
        None | Some("") => Some(StatoRichiesta::Aperta),
        Some("tutte") => None,
        Some(x) => Some(StatoRichiesta::da_testo(x)?),
    };
    json_db(&s, move |c| {
        Ok(json!({ "richieste": repo::elenca(c, stato)? }))
    })
    .await
}

/// `{esito: approvata|negata, sempre?, comando_approvato?, valori?}`: sveglia chi aspetta.
pub async fn risolvi(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Corpo(b): Corpo<CorpoRisolvi>,
) -> RisHttp<Value> {
    Ok(Json(richieste::risolvi(&s, &id, b).await?))
}
