//! Notifiche (Fase 2.A: solo lettura; il resto arriva con la Fase 2.D).

use rusqlite::Connection;
use serde_json::Value;
use xbot_core::Risposta;

use crate::util::tutte_json;

/// Le notifiche non lette, le più recenti prima.
pub fn non_lette(conn: &Connection) -> Risposta<Vec<Value>> {
    tutte_json(
        conn,
        "SELECT * FROM notifica WHERE letta_ms IS NULL ORDER BY creata_ms DESC LIMIT 200",
        [],
    )
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn elenco_vuoto_su_database_nuovo() {
        let db = crate::db_prova();
        db.con(|c| {
            assert!(non_lette(c)?.is_empty());
            Ok(())
        })
        .unwrap();
    }
}
