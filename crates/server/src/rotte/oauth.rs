//! Le rotte OAuth dei collegamenti (Fase 2.I):
//! - `POST /collegamenti/{id}/oauth/avvia` → `{url}` (dietro l'accesso);
//! - `GET /oauth/ritorno?code&state` → 303 su `/#impostazioni/collegamenti?oauth=ok|errore…`;
//!   resta dietro il cookie, ma senza cookie risponde con una pagina HTML, non con JSON;
//! - `DELETE /collegamenti/{id}/oauth` → scollega (revoca e token cancellati).

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum_extra::extract::cookie::SignedCookieJar;
use serde::Deserialize;
use serde_json::{Value, json};
use xbot_core::Codice;

use super::json_db;
use crate::errore_http::{Domanda, RisHttp};
use crate::stato::Stato;

/// L'indirizzo di ritorno: `XBOT_URL_PUBBLICO`, altrimenti l'host della richiesta.
fn ritorno_uri(s: &Stato, h: &HeaderMap) -> String {
    format!(
        "{}/api/v1/oauth/ritorno",
        super::catalogo::base_pubblica(s, h)
    )
}

pub async fn avvia(State(s): State<Stato>, Path(id): Path<String>, h: HeaderMap) -> RisHttp<Value> {
    let uri = ritorno_uri(&s, &h);
    let url = crate::oauth::avvia(
        &s.db,
        s.servizi.cassaforte.clone(),
        &s.oauth_tentativi,
        &id,
        &uri,
    )
    .await?;
    Ok(Json(json!({ "url": url })))
}

pub async fn scollega(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    crate::oauth::scollega(&s.db, s.servizi.cassaforte.clone(), &id).await?;
    json_db(&s, move |c| xbot_db::collegamenti::leggi(c, &id)).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpoClient {
    pub client_id: String,
    pub client_secret: Option<String>,
}

/// `PUT /collegamenti/{id}/oauth/client {client_id, client_secret?}`: per i server senza
/// registrazione dinamica (Slack). Il segreto va cifrato e non esce più.
pub async fn client(
    State(s): State<Stato>,
    Path(id): Path<String>,
    crate::errore_http::Corpo(b): crate::errore_http::Corpo<CorpoClient>,
) -> RisHttp<Value> {
    let k = s.servizi.cassaforte.clone();
    json_db(&s, move |c| {
        let segreto = b
            .client_secret
            .filter(|x| !x.trim().is_empty())
            .map(xbot_core::SegretoTesto::from);
        xbot_db::oauth::imposta_client(c, k.as_deref(), &id, &b.client_id, segreto.as_ref())?;
        xbot_db::collegamenti::leggi(c, &id)
    })
    .await
}

#[derive(Deserialize)]
pub struct Ritorno {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

fn vai(dove: &str) -> Response {
    (
        StatusCode::SEE_OTHER,
        [(header::LOCATION, dove.to_string())],
    )
        .into_response()
}

fn pagina(testo: &str) -> Response {
    let html = format!(
        "<!doctype html><html lang=\"it\"><meta charset=\"utf-8\"><meta name=\"viewport\" \
         content=\"width=device-width\"><title>XBot</title><body style=\"font-family:system-ui;\
         padding:2rem;max-width:32rem;margin:auto\"><h1>XBot</h1><p>{testo}</p>\
         <p><a href=\"/\">Apri XBot</a></p></body></html>"
    );
    (StatusCode::UNAUTHORIZED, Html(html)).into_response()
}

/// Il codice d'errore del ritorno: solo lettere, cifre e `_` (va in un URL).
fn motivo(codice: &str) -> String {
    codice
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(60)
        .collect()
}

pub async fn ritorno(
    State(s): State<Stato>,
    jar: SignedCookieJar,
    Domanda(r): Domanda<Ritorno>,
) -> Response {
    // Il browser che torna dal consenso è quello dell'utente: il cookie c'è.
    let Some(c) = jar.get(crate::accesso::COOKIE) else {
        return pagina("Accedi a XBot e ripeti il collegamento.");
    };
    let h = crate::accesso::hash_token(c.value());
    match s.db.esegui(move |c| xbot_db::accesso::valido(c, &h)).await {
        Ok(Some(_)) => {}
        _ => return pagina("Accedi a XBot e ripeti il collegamento."),
    }
    // Il collegamento del tentativo, se noto: W lo usa per dire il nome nell'avviso.
    let coll = r
        .state
        .as_deref()
        .and_then(|st| s.oauth_tentativi.collegamento_di(st))
        .map(|id| format!("&collegamento={}", motivo(&id)))
        .unwrap_or_default();
    if let Some(e) = r.error {
        tracing::warn!(errore = %motivo(&e), "OAuth rifiutato dal server di autorizzazione");
        if let Some(st) = r.state.as_deref() {
            s.oauth_tentativi.togli(st);
        }
        return vai(&format!(
            "/#impostazioni/collegamenti?oauth=errore&motivo={}{coll}",
            motivo(&e)
        ));
    }
    let (Some(code), Some(state)) = (r.code, r.state) else {
        return crate::errore_http::ErroreHttp(xbot_core::Errore::nuovo(
            Codice::OauthStatoNonValido,
            "mancano code e state",
        ))
        .into_response();
    };
    let esito = crate::oauth::ritorno(
        &s.db,
        s.servizi.cassaforte.clone(),
        &s.oauth_tentativi,
        &code,
        &state,
    )
    .await;
    match esito {
        Ok(id) => vai(&format!(
            "/#impostazioni/collegamenti?oauth=ok&collegamento={id}"
        )),
        Err(e) if e.codice == Codice::OauthStatoNonValido => {
            crate::errore_http::ErroreHttp(e).into_response()
        }
        Err(e) => {
            tracing::warn!(errore = %e, "OAuth: ritorno non riuscito");
            vai(&format!(
                "/#impostazioni/collegamenti?oauth=errore&motivo={}{coll}",
                e.codice.come_testo()
            ))
        }
    }
}
