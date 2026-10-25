//! `GET /push/chiave`, `POST`/`DELETE /push/abbonamento`.

use axum::extract::State;
use rusqlite::params;
use serde_json::{Value, json};
use xbot_core::{Codice, Errore, nuovo_id, ora_ms};

use super::json_db;
use crate::errore_http::{Corpo, RisHttp};
use crate::stato::Stato;

/// La chiave pubblica VAPID; 503 `push_non_configurato` se il server non ha le chiavi.
pub async fn chiave(State(s): State<Stato>) -> RisHttp<Value> {
    let v = s.servizi.vapid.clone().ok_or_else(|| {
        Errore::nuovo(
            Codice::PushNonConfigurato,
            "mancano XBOT_VAPID_PRIVATA e PUBBLICA sul server",
        )
    })?;
    Ok(axum::Json(json!({ "chiave": v.pubblica })))
}

/// `{endpoint, chiavi{p256dh, auth}, dispositivo}` (accetta anche `keys` come lo dà il browser).
pub async fn abbonati(State(s): State<Stato>, Corpo(b): Corpo<Value>) -> RisHttp<Value> {
    let (endpoint, chiavi, dispositivo) = crate::push::controlla_abbonamento(&b)?;
    json_db(&s, move |c| {
        c.execute(
            "INSERT INTO abbonamento_push (id, endpoint, chiavi_json, dispositivo, creato_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(endpoint) DO UPDATE SET chiavi_json = excluded.chiavi_json,
               dispositivo = excluded.dispositivo",
            params![
                nuovo_id(),
                endpoint,
                chiavi.to_string(),
                dispositivo,
                ora_ms()
            ],
        )
        .map_err(|e| Errore::database(e.to_string()))?;
        let id: String = c
            .query_row(
                "SELECT id FROM abbonamento_push WHERE endpoint = ?1",
                [&endpoint],
                |r| r.get(0),
            )
            .map_err(|e| Errore::database(e.to_string()))?;
        Ok(json!({ "id": id }))
    })
    .await
}

/// `{endpoint}`: toglie l'abbonamento di questo dispositivo.
pub async fn disdici(State(s): State<Stato>, Corpo(b): Corpo<Value>) -> RisHttp<Value> {
    let endpoint = b["endpoint"].as_str().unwrap_or("").to_string();
    json_db(&s, move |c| {
        let n = c
            .execute(
                "DELETE FROM abbonamento_push WHERE endpoint = ?1",
                [&endpoint],
            )
            .map_err(|e| Errore::database(e.to_string()))?;
        Ok(json!({ "tolti": n }))
    })
    .await
}
