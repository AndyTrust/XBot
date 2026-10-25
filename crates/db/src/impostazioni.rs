//! Impostazioni chiave/valore. Le chiavi con prefisso riservato non escono dall'API.

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Map, Value};
use xbot_core::{Codice, Errore, Risposta, ora_ms};

use crate::util::RisultatoDb;

/// Prefissi delle chiavi interne (hash della password, segreti del server).
pub const PREFISSI_RISERVATI: &[&str] = &["accesso.", "server."];

pub fn riservata(chiave: &str) -> bool {
    PREFISSI_RISERVATI.iter().any(|p| chiave.starts_with(p))
}

pub fn leggi(conn: &Connection, chiave: &str) -> Risposta<Option<Value>> {
    let t: Option<String> = conn
        .query_row(
            "SELECT valore_json FROM impostazione WHERE chiave = ?1",
            [chiave],
            |r| r.get(0),
        )
        .optional()
        .db()?;
    Ok(t.and_then(|t| serde_json::from_str(&t).ok()))
}

/// Scrive una chiave (anche riservata: solo per il codice del motore).
pub fn scrivi(conn: &Connection, chiave: &str, valore: &Value) -> Risposta<()> {
    conn.execute(
        "INSERT INTO impostazione (chiave, valore_json, aggiornata_ms) VALUES (?1, ?2, ?3)
         ON CONFLICT(chiave) DO UPDATE SET valore_json = excluded.valore_json,
           aggiornata_ms = excluded.aggiornata_ms",
        params![chiave, valore.to_string(), ora_ms()],
    )
    .db()?;
    Ok(())
}

/// Tutte le impostazioni pubbliche.
pub fn pubbliche(conn: &Connection) -> Risposta<Map<String, Value>> {
    let mut st = conn
        .prepare("SELECT chiave, valore_json FROM impostazione ORDER BY chiave")
        .db()?;
    let righe = st
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .db()?
        .collect::<rusqlite::Result<Vec<_>>>()
        .db()?;
    Ok(righe
        .into_iter()
        .filter(|(k, _)| !riservata(k))
        .map(|(k, v)| (k, serde_json::from_str(&v).unwrap_or(Value::Null)))
        .collect())
}

/// Scrive più impostazioni pubbliche; rifiuta le chiavi riservate.
pub fn scrivi_pubbliche(conn: &Connection, valori: &Map<String, Value>) -> Risposta<()> {
    if let Some(k) = valori.keys().find(|k| riservata(k) || k.is_empty()) {
        return Err(Errore::nuovo(
            Codice::ChiaveRiservata,
            format!("chiave non scrivibile: {k}"),
        ));
    }
    for k in valori.keys() {
        crate::controlli::lunghezza("chiave", Some(k), crate::controlli::CHIAVE_IMPOSTAZIONE)?;
    }
    crate::util::in_transazione(conn, || {
        for (k, v) in valori {
            scrivi(conn, k, v)?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json::json;

    #[test]
    fn riservate_nascoste_e_protette() {
        let db = crate::db_prova();
        db.con(|c| {
            scrivi(c, "accesso.password_hash", &json!("x"))?;
            let mut m = Map::new();
            m.insert("tema".into(), json!("scuro"));
            scrivi_pubbliche(c, &m)?;
            let p = pubbliche(c)?;
            assert_eq!(p.len(), 1);
            assert_eq!(p["tema"], "scuro");
            m.insert("server.segreto".into(), json!("y"));
            assert_eq!(
                scrivi_pubbliche(c, &m).unwrap_err().codice,
                Codice::ChiaveRiservata
            );
            assert_eq!(leggi(c, "accesso.password_hash")?, Some(json!("x")));
            Ok(())
        })
        .unwrap();
    }
}
