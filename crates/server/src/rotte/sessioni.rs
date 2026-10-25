//! `/assistenti/{id}/sessioni` e `/sessioni/{id}`: lettura, modifica, azzeramento, righe.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use xbot_core::{Codice, Errore};
use xbot_db::righe::{self, Pagina};
use xbot_db::sessioni::{self as repo, NuovaSessione, Sessione};

use super::json_db;
use crate::bus::Segnale;
use crate::errore_http::{Corpo, Domanda, RisHttp};
use crate::stato::Stato;

#[derive(Deserialize)]
pub struct FiltroSessioni {
    #[serde(default)]
    pub tutte: bool,
}

pub async fn elenca(
    State(s): State<Stato>,
    Path(assistente): Path<String>,
    Domanda(f): Domanda<FiltroSessioni>,
) -> RisHttp<Vec<Sessione>> {
    json_db(&s, move |c| {
        xbot_db::assistenti::leggi(c, &assistente)?;
        repo::elenca(c, &assistente, f.tutte)
    })
    .await
}

pub async fn crea(
    State(s): State<Stato>,
    Path(assistente): Path<String>,
    Corpo(n): Corpo<NuovaSessione>,
) -> RisHttp<Sessione> {
    json_db(&s, move |c| repo::crea(c, &assistente, &n)).await
}

pub async fn leggi(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Sessione> {
    json_db(&s, move |c| repo::leggi(c, &id)).await
}

/// Titolo, provider, modello, livello, macchina, cartella, `archivia: true|false`.
pub async fn aggiorna(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Corpo(m): Corpo<Map<String, Value>>,
) -> RisHttp<Sessione> {
    json_db(&s, move |c| repo::aggiorna(c, &id, &m)).await
}

/// Nuova generazione. Rifiutato con un turno in corso o in coda.
pub async fn azzera(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    if s.motore.occupata(&id) {
        return Err(Errore::nuovo(Codice::TurnoInCorso, "c'è un turno in corso").into());
    }
    let sid = id.clone();
    let generazione = s.db.esegui(move |c| repo::azzera(c, &sid)).await?;
    s.bus.pubblica(&id, Segnale::Azzerata { generazione });
    Ok(Json(json!({ "generazione": generazione })))
}

#[derive(Deserialize)]
pub struct Cursore {
    pub dopo_aggiornata_seq: Option<i64>,
    pub generazione: Option<i64>,
    pub limite: Option<i64>,
}

/// Lettura a cursore: righe con `aggiornata_seq` maggiore del cursore.
pub async fn righe(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Domanda(q): Domanda<Cursore>,
) -> RisHttp<Pagina> {
    json_db(&s, move |c| {
        righe::dopo(
            c,
            &id,
            q.generazione,
            q.dopo_aggiornata_seq.unwrap_or(0),
            q.limite.unwrap_or(200),
        )
    })
    .await
}
