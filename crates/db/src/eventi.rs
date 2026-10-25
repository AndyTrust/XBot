//! Diario di sistema (`evento_sistema`): avvii, errori dei provider, ecc. Mai testo dei messaggi.

use rusqlite::{Connection, params};
use serde_json::Value;
use xbot_core::{Risposta, ora_ms};

use crate::util::{RisultatoDb, tutte_json};

pub fn scrivi(conn: &Connection, tipo: &str, dettaglio: &Value) -> Risposta<()> {
    conn.execute(
        "INSERT INTO evento_sistema (tipo, dettaglio_json, ms) VALUES (?1, ?2, ?3)",
        params![tipo, dettaglio.to_string(), ora_ms()],
    )
    .db()?;
    Ok(())
}

pub fn ultimi(conn: &Connection, quanti: i64) -> Risposta<Vec<Value>> {
    tutte_json(
        conn,
        "SELECT * FROM evento_sistema ORDER BY ms DESC, id DESC LIMIT ?1",
        [quanti.clamp(1, 1000)],
    )
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn scrive_e_rilegge() {
        let db = crate::db_prova();
        db.con(|c| {
            scrivi(c, "avvio", &serde_json::json!({"versione": "0.1.0"}))?;
            let v = ultimi(c, 10)?;
            assert_eq!(v[0]["tipo"], "avvio");
            assert_eq!(v[0]["dettaglio_json"]["versione"], "0.1.0");
            Ok(())
        })
        .unwrap();
    }
}
