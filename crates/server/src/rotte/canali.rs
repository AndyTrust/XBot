//! `/canali` (Fase 2.D S2): i client MCP esterni (altri bot e agenti) con il loro token. Il token si
//! vede una volta sola (alla creazione e a ogni rigenerazione); nel database c'è il suo SHA-256.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use xbot_core::nuovo_id;
use xbot_db::canali as repo;

use super::json_db;
use crate::errore_http::{Corpo, Domanda, ErroreHttp, RisHttp};
use crate::stato::Stato;

/// Il prefisso dei token dei canali (per riconoscerli a colpo d'occhio).
pub const PREFISSO: &str = "plc_";

/// Lo SHA-256 esadecimale di un token.
pub fn hash_token(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn token_nuovo() -> String {
    let b: [u8; 32] = rand::random();
    format!(
        "{PREFISSO}{}",
        b.iter().map(|x| format!("{x:02x}")).collect::<String>()
    )
}

fn url_mcp(s: &Stato) -> String {
    match &s.servizi.url_pubblico {
        Some(u) => format!("{}/mcp", u.trim_end_matches('/')),
        None => "/mcp".into(),
    }
}

pub async fn elenca(State(s): State<Stato>) -> RisHttp<Value> {
    json_db(&s, |c| Ok(json!({ "canali": repo::elenca(c)? }))).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NuovoCanale {
    pub nome: String,
    pub assistente_id: Option<String>,
    #[serde(default)]
    pub strumenti: Vec<String>,
}

/// `POST /canali` → `{id, token, url}`.
pub async fn crea(State(s): State<Stato>, Corpo(n): Corpo<NuovoCanale>) -> RisHttp<Value> {
    let (id, token) = (nuovo_id(), token_nuovo());
    let (id2, h) = (id.clone(), hash_token(&token));
    s.db.esegui(move |c| {
        repo::crea(
            c,
            &id2,
            &n.nome,
            n.assistente_id.as_deref().filter(|a| !a.is_empty()),
            &n.strumenti,
            &h,
        )
    })
    .await?;
    tracing::info!(canale = %id, "canale creato");
    Ok(Json(
        json!({ "id": id, "token": token, "url": url_mcp(&s) }),
    ))
}

/// `DELETE /canali/{id}`: revoca.
pub async fn revoca(
    State(s): State<Stato>,
    Path(id): Path<String>,
) -> Result<StatusCode, ErroreHttp> {
    s.db.esegui(move |c| repo::revoca(c, &id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `PATCH /canali/{id}`: `nome`, `assistente_id`, `strumenti`, `limite_orario` → il canale.
pub async fn aggiorna(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Corpo(m): Corpo<serde_json::Map<String, Value>>,
) -> RisHttp<Value> {
    json_db(&s, move |c| {
        repo::aggiorna(c, &id, &m)?;
        repo::elenca(c)?
            .into_iter()
            .find(|k| k["id"] == id.as_str())
            .ok_or_else(|| {
                xbot_core::Errore::nuovo(xbot_core::Codice::NonTrovato, "canale non trovato")
            })
    })
    .await
}

/// `POST /canali/{id}/rigenera` → `{id, token, url}` (il token vecchio non vale più).
pub async fn rigenera(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    let token = token_nuovo();
    let (id2, h) = (id.clone(), hash_token(&token));
    s.db.esegui(move |c| repo::rigenera(c, &id2, &h)).await?;
    Ok(Json(
        json!({ "id": id, "token": token, "url": url_mcp(&s) }),
    ))
}

#[derive(Deserialize)]
pub struct FiltroChiamate {
    pub limite: Option<i64>,
}

pub async fn chiamate(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Domanda(f): Domanda<FiltroChiamate>,
) -> RisHttp<Value> {
    json_db(&s, move |c| {
        Ok(json!({ "chiamate": repo::chiamate(c, &id, f.limite.unwrap_or(50))? }))
    })
    .await
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn token_e_hash() {
        let t = token_nuovo();
        assert!(t.starts_with(PREFISSO) && t.len() == PREFISSO.len() + 64);
        assert_eq!(
            hash_token("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_ne!(token_nuovo(), t);
    }
}
