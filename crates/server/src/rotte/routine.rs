//! `/assistenti/{id}/routine` e `/routine/{id}`: elenco, crea, modifica, elimina, esegui.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use xbot_core::{Errore, ora_ms};
use xbot_db::routine::{self as repo, NuovaRoutine};

use super::json_db;
use crate::errore_http::{Corpo, ErroreHttp, RisHttp};
use crate::orologio;
use crate::stato::Stato;

pub async fn elenca(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    json_db(&s, move |c| {
        xbot_db::assistenti::leggi(c, &id)?;
        Ok(json!({ "routine": repo::elenca(c, Some(&id))? }))
    })
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpoRoutine {
    pub nome: String,
    pub descrizione: Option<String>,
    pub corpo: String,
    pub attivatore_tipo: String,
    pub attivatore_json: Value,
    #[serde(default = "vero")]
    pub abilitata: bool,
}

fn vero() -> bool {
    true
}

pub async fn crea(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Corpo(b): Corpo<CorpoRoutine>,
) -> RisHttp<Value> {
    let attivatore = orologio::controlla(&b.attivatore_tipo, &b.attivatore_json)?;
    let prossima_ms = orologio::prossima(&b.attivatore_tipo, &attivatore, ora_ms())?;
    let n = NuovaRoutine {
        nome: b.nome,
        descrizione: b.descrizione,
        corpo: b.corpo,
        attivatore_tipo: b.attivatore_tipo,
        attivatore,
        abilitata: b.abilitata,
        prossima_ms,
    };
    json_db(&s, move |c| repo::crea(c, &id, &n)).await
}

pub async fn leggi(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    json_db(&s, move |c| repo::leggi(c, &id)).await
}

/// Modifica parziale: `nome`, `descrizione`, `corpo`, `attivatore_tipo`, `attivatore_json`,
/// `abilitata`. Se cambia l'attivatore (o si riabilita) la scadenza si ricalcola da adesso.
pub async fn aggiorna(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Corpo(mut m): Corpo<Map<String, Value>>,
) -> RisHttp<Value> {
    let rid = id.clone();
    let attuale = s.db.esegui(move |c| repo::leggi(c, &rid)).await?;
    let ricalcola = m.contains_key("attivatore_tipo")
        || m.contains_key("attivatore_json")
        || m.get("abilitata") == Some(&Value::Bool(true));
    if ricalcola {
        let tipo = m
            .get("attivatore_tipo")
            .or(attuale.get("attivatore_tipo"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let j = m
            .get("attivatore_json")
            .cloned()
            .unwrap_or_else(|| attuale["attivatore_json"].clone());
        let j = orologio::controlla(&tipo, &j)?;
        let prossima = orologio::prossima(&tipo, &j, ora_ms())?;
        m.insert("attivatore_json".into(), Value::from(j.to_string()));
        m.insert("prossima_ms".into(), json!(prossima));
    }
    if m.contains_key("prossima_ms") && !ricalcola {
        return Err(Errore::non_valida("campo non modificabile: prossima_ms").into());
    }
    // Una routine spenta non ha una prossima esecuzione.
    if matches!(m.get("abilitata"), Some(Value::Bool(false)))
        || m.get("abilitata") == Some(&json!(0))
    {
        m.insert("prossima_ms".into(), Value::Null);
    }
    json_db(&s, move |c| repo::aggiorna(c, &id, &m)).await
}

pub async fn elimina(
    State(s): State<Stato>,
    Path(id): Path<String>,
) -> Result<StatusCode, ErroreHttp> {
    s.db.esegui(move |c| repo::elimina(c, &id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Esegue la routine adesso (la scadenza non cambia).
pub async fn esegui(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    let r = s.db.esegui(move |c| repo::leggi(c, &id)).await?;
    xbot_db::controlli::lunghezza("corpo", r["corpo"].as_str(), repo::MAX_CORPO)?;
    Ok(Json(orologio::esegui(&s, &r, false).await?))
}

pub async fn esecuzioni(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    json_db(&s, move |c| {
        repo::leggi(c, &id)?;
        Ok(json!({ "esecuzioni": repo::esecuzioni(c, &id, 50)? }))
    })
    .await
}
