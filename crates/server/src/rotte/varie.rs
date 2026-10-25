//! `/salute`, `/io`, `/barra`, `/impostazioni`, `/letto` (`/provider`, `/modelli`, `/quote` dalla
//! 2.I in `provider.rs`).

use axum::Json;
use axum::extract::{Extension, Path, State};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use xbot_core::{Codice, Errore, VERSIONE};
use xbot_db::accesso::AccessoWeb;
use xbot_db::{impostazioni, lettura, provider, sezioni};

use super::json_db;
use crate::errore_http::{Corpo, ErroreHttp, RisHttp};
use crate::stato::Stato;

/// Senza accesso: il motore risponde e il database si legge.
pub async fn salute(State(s): State<Stato>) -> Json<Value> {
    let installata = xbot_core::versione_installata(&s.servizi.cartella_dati);
    match s.db.esegui(xbot_db::versione_schema).await {
        Ok(v) => Json(
            json!({ "ok": true, "versione": VERSIONE, "nome_app": xbot_core::nome_bot(),
                         "versione_installata": installata,
                         "db": { "ok": true, "schema": v } }),
        ),
        Err(e) => Json(
            json!({ "ok": false, "versione": VERSIONE, "nome_app": xbot_core::nome_bot(),
                    "versione_installata": installata,
                    "db": { "ok": false, "codice": e.codice } }),
        ),
    }
}

pub async fn io(State(s): State<Stato>, Extension(a): Extension<AccessoWeb>) -> RisHttp<Value> {
    json_db(&s, move |c| {
        let nome = xbot_db::accesso::utente(c)?.map(|(n, _)| n);
        Ok(json!({
            "nome": nome,
            "versione": VERSIONE,
            "accesso": a,
            "impostazioni": impostazioni::pubbliche(c)?,
            "provider": provider_senza_programma(provider::elenca(c)?),
            "collegamenti_accesi": xbot_db::collegamenti::accesi(c)?,
        }))
    })
    .await
}

/// In `/io` il percorso del programma del provider non esce: restano id e nome.
fn provider_senza_programma(mut v: Vec<Value>) -> Vec<Value> {
    for p in &mut v {
        if let Some(o) = p.as_object_mut() {
            o.remove("programma");
        }
    }
    v
}

pub async fn barra(State(s): State<Stato>) -> RisHttp<Vec<Value>> {
    json_db(&s, sezioni::barra).await
}

/// Il contratto di `/impostazioni`: `{"valori": {chiave: valore_json, …}}` in lettura e in
/// scrittura. Le chiavi riservate (`accesso.*`, `server.*`) non escono e non si scrivono (403).
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Valori {
    pub valori: Map<String, Value>,
}

pub async fn impostazioni(State(s): State<Stato>) -> RisHttp<Valori> {
    json_db(&s, |c| {
        Ok(Valori {
            valori: impostazioni::pubbliche(c)?,
        })
    })
    .await
}

/// Scrive solo le chiavi date (le altre restano) e restituisce tutte le pubbliche.
pub async fn scrivi_impostazioni(
    State(s): State<Stato>,
    Corpo(v): Corpo<Valori>,
) -> RisHttp<Valori> {
    json_db(&s, move |c| {
        impostazioni::scrivi_pubbliche(c, &v.valori)?;
        Ok(Valori {
            valori: impostazioni::pubbliche(c)?,
        })
    })
    .await
}

#[derive(Deserialize)]
pub struct Letto {
    /// Dalla migrazione 002 i non letti contano per tempo (`ultima_vista_ms`);
    /// `fino_seq` resta per compatibilità.
    #[serde(default)]
    pub fino_seq: i64,
}

pub async fn letto(
    State(s): State<Stato>,
    Path(assistente): Path<String>,
    Corpo(l): Corpo<Letto>,
) -> RisHttp<Value> {
    let (bus, aid) = (s.bus.clone(), assistente.clone());
    let sessioni =
        s.db.esegui(move |c| {
            xbot_db::assistenti::leggi(c, &assistente)?;
            lettura::segna_letto(c, &assistente, l.fino_seq)?;
            xbot_db::sessioni::elenca(c, &assistente, false)
        })
        .await?;
    // --- S2 --- frame `barra` sui flussi delle sessioni dell'assistente.
    for x in sessioni {
        bus.pubblica(&x.id, crate::bus::Segnale::Barra);
    }
    tracing::debug!(assistente = %aid, "letto");
    Ok(axum::Json(json!({ "ok": true })))
}

/// Le rotte API che non esistono rispondono in JSON.
pub async fn non_trovata() -> ErroreHttp {
    ErroreHttp(Errore::nuovo(Codice::NonTrovato, "rotta non trovata"))
}
