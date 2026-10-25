//! Fase 2.L, difetto 6: una quota letta prima che la sua finestra si azzerasse non è più vera.
//! `GET /quote` dà `scaduta: true` quando `azzera_ms` è passato (anche per ogni finestra di
//! `dettaglio_json`); all'avvio e ogni 10 minuti il server richiede la quota ai client che sanno
//! darla senza un turno (`Provider::quota`). Un turno «sonda» non si fa: Claude Code la dichiara
//! solo dentro un turno, quindi la sua resta `scaduta` fino al turno dopo.

use std::time::Duration;

use serde_json::Value;
use xbot_core::{Ms, ora_ms};

use crate::stato::Stato;

/// Ogni quanto si rileggono le quote.
pub const OGNI: Duration = Duration::from_secs(600);

fn passato(v: &Value, ora: Ms) -> bool {
    v["azzera_ms"].as_i64().is_some_and(|a| a <= ora)
}

/// Aggiunge `scaduta` alla riga di `GET /quote` e alle sue finestre (`dettaglio_json`, anche
/// quando arriva come testo).
pub fn segna(riga: &mut Value, ora: Ms) {
    let s = passato(riga, ora);
    let Some(o) = riga.as_object_mut() else {
        return;
    };
    o.insert("scaduta".into(), Value::Bool(s));
    let Some(d) = o.get_mut("dettaglio_json") else {
        return;
    };
    if let Some(v) = d
        .as_str()
        .and_then(|t| serde_json::from_str::<Value>(t).ok())
    {
        *d = v;
    }
    if let Some(f) = d.as_object_mut() {
        for w in f.values_mut() {
            let s = passato(w, ora);
            if let Some(w) = w.as_object_mut() {
                w.insert("scaduta".into(), Value::Bool(s));
            }
        }
    }
}

/// Le righe di `GET /quote` con `scaduta`.
pub fn segna_tutte(v: &mut [Value]) {
    let ora = ora_ms();
    v.iter_mut().for_each(|r| segna(r, ora));
}

/// Una rilettura: per ogni provider caricato qui che dà la quota senza un turno, una
/// rilevazione nuova (fonte `client`). Restituisce quante.
pub async fn rileggi(s: &Stato) -> usize {
    let mut n = 0;
    for id in s.registro.ids() {
        let Some(p) = s.registro.trova(&id) else {
            continue;
        };
        match p.quota().await {
            Ok(Some(q)) => {
                let r =
                    s.db.esegui(move |c| {
                        xbot_db::provider::rileva_quota(
                            c,
                            &id,
                            q.percentuale_usata,
                            q.azzera_ms,
                            "client",
                            None,
                        )
                    })
                    .await;
                match r {
                    Ok(()) => n += 1,
                    Err(e) => tracing::warn!(errore = %e, "quota riletta non scritta"),
                }
            }
            Ok(None) => {}
            Err(e) => tracing::info!(provider = %id, errore = %e, "quota non riletta"),
        }
    }
    n
}

/// All'avvio e ogni [`OGNI`].
pub fn avvia(s: Stato) {
    tokio::spawn(async move {
        let mut t = tokio::time::interval(OGNI);
        loop {
            t.tick().await;
            let n = rileggi(&s).await;
            tracing::debug!(rilette = n, "quote rilette");
        }
    });
}
