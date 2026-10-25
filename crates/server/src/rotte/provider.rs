//! `/provider`, `/modelli`, `/quote` (spostate da `varie.rs` nella 2.I) e l'accesso ai client:
//! `POST /provider/{id}/accedi|esci`, `GET|POST /provider/{id}/accesso`, `PATCH /provider/{id}`.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use xbot_db::provider;

use super::json_db;
use crate::errore_http::{Corpo, Domanda, RisHttp};
use crate::stato::Stato;

pub async fn quote(State(s): State<Stato>) -> RisHttp<Vec<Value>> {
    let mut v = s.db.esegui(provider::quote).await?;
    // --- 2.L --- `scaduta` quando la finestra si è già azzerata.
    crate::quote_scadute::segna_tutte(&mut v);
    Ok(Json(v))
}

/// I provider del database, con `caricato` (adattatore in questo motore), le macchine dove
/// girano (`account`, `versione`, `in_linea`), `riceve_collegamenti` e `quota_fonte`.
pub async fn elenca(State(s): State<Stato>) -> RisHttp<Vec<Value>> {
    let ids = s.registro.ids();
    let server = s.servizi.macchina.id.clone();
    let connesse = s.motore.macchine().connesse();
    json_db(&s, move |c| {
        let mut v = provider::elenca(c)?;
        let dove = provider::macchine_per_provider(c, &server, &ids)?;
        for p in &mut v {
            let id = p["id"].as_str().unwrap_or("").to_string();
            p["caricato"] = Value::Bool(ids.contains(&id));
            let mut macchine = dove.get(&id).cloned().unwrap_or_default();
            for m in &mut macchine {
                let mid = m["id"].as_str().unwrap_or("").to_string();
                let e_server = mid == server;
                m["e_il_server"] = Value::Bool(e_server);
                m["in_linea"] = Value::Bool(e_server || connesse.contains(&mid));
            }
            p["macchine"] = Value::Array(macchine);
            p["riceve_collegamenti"] = Value::Bool(xbot_provider::riceve_collegamenti(&id));
            p["quota_fonte"] = json!(provider::fonte_quota(c, &id)?);
        }
        Ok(v)
    })
    .await
}

#[derive(Deserialize)]
pub struct FiltroModelli {
    pub provider: Option<String>,
}

pub async fn modelli(
    State(s): State<Stato>,
    Domanda(f): Domanda<FiltroModelli>,
) -> RisHttp<Vec<Value>> {
    let (ids, server) = (s.registro.ids(), s.servizi.macchina.id.clone());
    json_db(&s, move |c| {
        let dove = provider::macchine_per_provider(c, &server, &ids)?;
        let spenti: Vec<Value> = provider::elenca(c)?
            .into_iter()
            .filter(|p| p["attivo"] == 0)
            .map(|p| p["id"].clone())
            .collect();
        let mut v = provider::modelli(c, f.provider.as_deref())?;
        for m in &mut v {
            let p = m["provider_id"].as_str().unwrap_or("");
            m["macchine"] = Value::Array(dove.get(p).cloned().unwrap_or_default());
            // --- 2.I --- un provider spento non va nella scelta del modello.
            m["provider_attivo"] = Value::Bool(!spenti.contains(&m["provider_id"]));
        }
        Ok(v)
    })
    .await
}

/// Il corpo di `accedi` ed `esci`: la macchina (predefinita il server).
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct CorpoMacchina {
    pub macchina_id: Option<String>,
}

fn macchina(s: &Stato, m: Option<String>) -> String {
    m.filter(|m| !m.is_empty())
        .unwrap_or_else(|| s.servizi.macchina.id.clone())
}

async fn esiste(s: &Stato, id: &str) -> Result<(), crate::errore_http::ErroreHttp> {
    let id = id.to_string();
    s.db.esegui(move |c| {
        let ok = provider::elenca(c)?.iter().any(|p| p["id"] == id.as_str());
        if ok {
            Ok(())
        } else {
            Err(xbot_core::Errore::nuovo(
                xbot_core::Codice::ProviderNonTrovato,
                "provider non trovato",
            ))
        }
    })
    .await?;
    Ok(())
}

pub async fn accedi(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Corpo(b): Corpo<CorpoMacchina>,
) -> RisHttp<Value> {
    esiste(&s, &id).await?;
    let m = macchina(&s, b.macchina_id);
    Ok(Json(crate::accessi::accedi(&s, &id, &m).await?))
}

pub async fn esci(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Corpo(b): Corpo<CorpoMacchina>,
) -> RisHttp<Value> {
    esiste(&s, &id).await?;
    let m = macchina(&s, b.macchina_id);
    Ok(Json(crate::accessi::esci(&s, &id, &m).await?))
}

#[derive(Deserialize)]
pub struct FiltroAccesso {
    pub macchina_id: Option<String>,
}

/// `GET /provider/{id}/accesso?macchina_id=` → `{stato, url, codice_utente, account, …}`.
pub async fn accesso(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Domanda(f): Domanda<FiltroAccesso>,
) -> RisHttp<Value> {
    let m = macchina(&s, f.macchina_id);
    let mut v = s.accessi.stato(&id, &m);
    v["macchina_id"] = json!(m);
    Ok(Json(v))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpoCodice {
    pub macchina_id: Option<String>,
    pub codice: String,
}

/// `POST /provider/{id}/accesso {macchina_id, codice}`: il codice da incollare (Claude Code).
pub async fn codice(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Corpo(b): Corpo<CorpoCodice>,
) -> RisHttp<Value> {
    let m = macchina(&s, b.macchina_id);
    crate::accessi::codice(&s, &id, &m, &b.codice).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpoProvider {
    pub attivo: bool,
}

/// `PATCH /provider/{id} {attivo}`: spento non compare nella scelta e «Auto» lo salta.
pub async fn aggiorna(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Corpo(b): Corpo<CorpoProvider>,
) -> RisHttp<Value> {
    esiste(&s, &id).await?;
    json_db(&s, move |c| {
        provider::imposta_attivo(c, &id, b.attivo)?;
        xbot_db::eventi::scrivi(
            c,
            "provider_attivo",
            &json!({ "provider_id": id, "attivo": b.attivo }),
        )?;
        Ok(json!({ "id": id, "attivo": b.attivo }))
    })
    .await
}
