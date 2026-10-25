//! `/collegamenti` (Fase 2.D S2): i server MCP che gli assistenti usano nei turni. La prova fa
//! `initialize` e `tools/list` con `xbot_provider::prova_mcp`, qui o (2.K) sul computer dove il
//! collegamento gira (`crate::prova_remota`).

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use xbot_db::collegamenti::{self as repo, NuovoCollegamento, StrumentoNoto};

use super::json_db;
use crate::errore_http::{Corpo, ErroreHttp, RisHttp};
use crate::stato::Stato;

fn chiave(s: &Stato) -> Option<std::sync::Arc<xbot_db::cassaforte::Chiave>> {
    s.servizi.cassaforte.clone()
}

pub async fn elenca(State(s): State<Stato>, h: HeaderMap) -> RisHttp<Value> {
    let base = super::catalogo::base_pubblica(&s, &h);
    let mut v = s.db.esegui(repo::elenca).await?;
    v.iter_mut()
        .for_each(|r| super::catalogo::arricchisci(&base, r));
    Ok(Json(json!({ "collegamenti": v })))
}

pub async fn crea(
    State(s): State<Stato>,
    Corpo(mut n): Corpo<NuovoCollegamento>,
) -> RisHttp<Value> {
    let k = chiave(&s);
    // --- 2.I --- le voci OAuth del catalogo nascono con `richiede_oauth`.
    if n.richiede_oauth.is_none() && super::catalogo::richiede_oauth(n.url.as_deref()) {
        n.richiede_oauth = Some(true);
    }
    json_db(&s, move |c| {
        Ok(json!({ "id": repo::crea(c, k.as_deref(), &n)? }))
    })
    .await
}

pub async fn leggi(State(s): State<Stato>, Path(id): Path<String>, h: HeaderMap) -> RisHttp<Value> {
    let mut v = s.db.esegui(move |c| repo::leggi(c, &id)).await?;
    super::catalogo::arricchisci(&super::catalogo::base_pubblica(&s, &h), &mut v);
    Ok(Json(v))
}

pub async fn aggiorna(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Corpo(m): Corpo<Map<String, Value>>,
) -> RisHttp<Value> {
    let k = chiave(&s);
    json_db(&s, move |c| {
        repo::aggiorna(c, k.as_deref(), &id, &m)?;
        repo::leggi(c, &id)
    })
    .await
}

pub async fn elimina(
    State(s): State<Stato>,
    Path(id): Path<String>,
) -> Result<StatusCode, ErroreHttp> {
    s.db.esegui(move |c| repo::elimina(c, &id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpoEsclusi {
    pub esclusi: Vec<String>,
}

/// 2.L: `PUT|POST /collegamenti/{id}/esclusi {esclusi: [id]}`: i bot per cui il collegamento è
/// spento (tutti gli altri lo usano).
pub async fn esclusi(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Corpo(b): Corpo<CorpoEsclusi>,
) -> RisHttp<Value> {
    json_db(&s, move |c| {
        repo::imposta_esclusi(c, &id, &b.esclusi)?;
        repo::leggi(c, &id)
    })
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpoPermessi {
    pub permessi: Map<String, Value>,
}

pub async fn permessi(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Corpo(b): Corpo<CorpoPermessi>,
) -> RisHttp<Value> {
    json_db(&s, move |c| {
        repo::imposta_permessi(c, &id, &b.permessi)?;
        repo::leggi(c, &id)
    })
    .await
}

/// `POST /collegamenti/{id}/prova` → `{ok, strumenti, errore?}`. Una prova fallita è una
/// risposta 200 con `ok: false` (il collegamento esiste, è lui a non rispondere). Dalla 2.I: prima
/// il rinnovo del token; senza accesso valido `ok: false` senza chiamare il server; un 401 con un
/// token valido → un rinnovo forzato e un secondo tentativo.
pub async fn prova(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    let k = chiave(&s);
    let _ = crate::oauth::rinnova(&s.db, k.clone(), &id, false).await;
    let mut esito = una_prova(&s, &id).await?;
    if let (Err(e), Some("ok")) = (&esito.0, esito.1.as_deref())
        && crate::oauth::chiede_accesso(e)
        && crate::oauth::rinnova(&s.db, k, &id, true).await? == crate::oauth::Rinnovo::Rinnovato
    {
        esito = una_prova(&s, &id).await?;
    }
    let esito = esito.0;
    let (id2, e2) = (id.clone(), esito.clone());
    s.db.esegui(move |c| repo::segna_prova(c, &id2, e2.as_deref().map_err(String::as_str)))
        .await?;
    tracing::info!(collegamento = %id, ok = esito.is_ok(), "prova del collegamento");
    Ok(Json(match esito {
        Ok(v) => json!({ "ok": true, "strumenti": v }),
        Err(e) => json!({ "ok": false, "strumenti": [], "errore": e }),
    }))
}

type EsitoProva = (Result<Vec<StrumentoNoto>, String>, Option<String>);

/// Una prova: (esito, stato OAuth al momento della prova).
async fn una_prova(s: &Stato, id: &str) -> Result<EsitoProva, ErroreHttp> {
    let k = chiave(s);
    let id2 = id.to_string();
    let vivo =
        s.db.esegui(move |c| repo::per_prova(c, k.as_deref(), &id2))
            .await?;
    let stato = vivo.oauth.clone();
    if !vivo.pronto() {
        let e = if stato.as_deref() == Some("scaduto") {
            crate::oauth::SCADUTO
        } else {
            crate::oauth::SERVE_ACCESSO
        };
        return Ok((Err(e.to_string()), stato));
    }
    // --- 2.K --- sulla macchina dove gira (lavoratore) o qui.
    let esito = crate::prova_remota::prova(s, &vivo).await;
    // Un 401 senza accesso OAuth: la scoperta dice che serve (2.I).
    if let Err(e) = &esito
        && stato.is_none()
        && vivo.trasporto == "http"
        && (crate::oauth::chiede_accesso(e)
            || crate::prova_remota::dichiara_oauth(vivo.url.as_deref().unwrap_or_default()).await)
    {
        let id2 = id.to_string();
        s.db.esegui(move |c| xbot_db::oauth::segna_richiede(c, &id2, None))
            .await?;
        return Ok((Err(crate::oauth::SERVE_ACCESSO.to_string()), stato));
    }
    Ok((esito, stato))
}
