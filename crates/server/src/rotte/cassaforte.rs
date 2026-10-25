//! `/cassaforte`: elenco, crea, modifica, elimina, rivela, importa CSV, usi.
//! Con la cassaforte chiusa ogni rotta risponde 503 `cassaforte_chiusa`.

use axum::extract::multipart::{Multipart, MultipartRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use xbot_core::GenereCassaforte;
use xbot_db::cassaforte_csv;
use xbot_db::cassaforte_voci::{self as voci, DatiVoce};

use super::allegati::leggi_file;
use super::json_db;
use crate::errore_http::{Corpo, Domanda, ErroreHttp, RisHttp};
use crate::stato::Stato;

#[derive(Deserialize)]
pub struct FiltroCassaforte {
    pub genere: Option<String>,
    pub cerca: Option<String>,
}

pub async fn elenca(
    State(s): State<Stato>,
    Domanda(f): Domanda<FiltroCassaforte>,
) -> RisHttp<Value> {
    s.servizi.chiave()?;
    let genere = f
        .genere
        .filter(|g| !g.is_empty())
        .map(|g| GenereCassaforte::da_testo(&g))
        .transpose()?;
    json_db(&s, move |c| {
        Ok(json!({ "voci": voci::cerca(c, genere, f.cerca.as_deref())? }))
    })
    .await
}

/// Il corpo di `POST /cassaforte/cerca`: il testo cercato viaggia nel corpo, mai nell'indirizzo
/// (che finirebbe nei log).
#[derive(Deserialize)]
pub struct CercaCassaforte {
    pub testo: Option<String>,
    pub genere: Option<String>,
}

pub async fn cerca(State(s): State<Stato>, Corpo(f): Corpo<CercaCassaforte>) -> RisHttp<Value> {
    s.servizi.chiave()?;
    let genere = f
        .genere
        .filter(|g| !g.is_empty())
        .map(|g| GenereCassaforte::da_testo(&g))
        .transpose()?;
    json_db(&s, move |c| {
        Ok(json!({ "voci": voci::cerca(c, genere, f.testo.as_deref())? }))
    })
    .await
}

pub async fn crea(State(s): State<Stato>, Corpo(d): Corpo<DatiVoce>) -> RisHttp<Value> {
    let k = s.servizi.chiave()?;
    json_db(&s, move |c| Ok(json!({ "id": voci::crea(c, &k, &d)? }))).await
}

pub async fn leggi(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    s.servizi.chiave()?;
    json_db(&s, move |c| voci::leggi(c, &id)).await
}

pub async fn aggiorna(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Corpo(d): Corpo<DatiVoce>,
) -> RisHttp<Value> {
    let k = s.servizi.chiave()?;
    json_db(&s, move |c| voci::aggiorna(c, &k, &id, &d)).await
}

pub async fn elimina(
    State(s): State<Stato>,
    Path(id): Path<String>,
) -> Result<StatusCode, ErroreHttp> {
    s.servizi.chiave()?;
    s.db.esegui(move |c| voci::elimina(c, &id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// I campi in chiaro, solo per l'utente dall'interfaccia (registra l'uso `riferimento`).
pub async fn rivela(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    let k = s.servizi.chiave()?;
    tracing::info!(voce = %id, "cassaforte: voce rivelata all'utente");
    json_db(&s, move |c| voci::rivela(c, &k, &id)).await
}

pub async fn usi(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    s.servizi.chiave()?;
    json_db(&s, move |c| Ok(json!({ "usi": voci::usi(c, &id)? }))).await
}

pub const MAX_CSV: usize = 8 * 1024 * 1024;

/// Multipart `file`: CSV di Bitwarden, LastPass, Firefox, 1Password, Safari/Apple, Chrome/Edge.
pub async fn importa(
    State(s): State<Stato>,
    mp: Result<Multipart, MultipartRejection>,
) -> RisHttp<Value> {
    let k = s.servizi.chiave()?;
    let f = leggi_file(mp, "file", MAX_CSV).await?;
    let r =
        s.db.esegui(move |c| cassaforte_csv::importa(c, &k, &f.byte))
            .await?;
    tracing::info!(
        importati = r.importati,
        saltati = r.saltati,
        errori = r.errori.len(),
        "cassaforte: CSV importato"
    );
    Ok(axum::Json(json!({
        "formato": r.formato, "importati": r.importati, "saltati": r.saltati, "errori": r.errori,
    })))
}
