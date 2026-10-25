//! 2.L-bis: gli avvisi che si ripetono uguali a ogni turno (`permessi_della_macchina`,
//! `client_senza_mcp`, `collegamento_senza_accesso`, `collegamento_altrove`) si scrivono una
//! volta per sessione: se la sessione ha già un avviso con lo stesso codice e lo stesso testo,
//! non se ne scrive un altro.

use serde_json::Value;
use xbot_core::GenereRiga;

use super::Motore;

/// Vero se la sessione ha già un avviso con quel codice e quel testo.
fn gia_detto(
    c: &rusqlite::Connection,
    sid: &str,
    codice: &str,
    testo: &str,
) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM riga WHERE sessione_id = ?1 AND genere = 'avviso'
           AND soppressa = 0 AND json_extract(corpo_json, '$.codice') = ?2
           AND json_extract(corpo_json, '$.testo') = ?3)",
        [sid, codice, testo],
        |r| r.get(0),
    )
}

impl Motore {
    /// La riga `avviso` del corpo, solo se la sessione non l'ha già.
    pub(in crate::motore) async fn avviso_una_volta(&self, sessione_id: &str, corpo: Value) {
        let codice = corpo["codice"].as_str().unwrap_or_default().to_string();
        let testo = corpo["testo"].as_str().unwrap_or_default().to_string();
        let sid = sessione_id.to_string();
        let detto = self
            .db
            .esegui(move |c| {
                gia_detto(c, &sid, &codice, &testo)
                    .map_err(|e| xbot_core::Errore::interno(format!("avvisi della sessione: {e}")))
            })
            .await
            .unwrap_or(false);
        if !detto {
            self.riga_sistema(sessione_id, GenereRiga::Avviso, corpo)
                .await;
        }
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn stesso_codice_e_testo() {
        let c = rusqlite::Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE riga (sessione_id TEXT, genere TEXT, soppressa INTEGER, corpo_json TEXT);
             INSERT INTO riga VALUES ('S', 'avviso', 0, '{\"codice\":\"a\",\"testo\":\"t\"}');",
        )
        .unwrap();
        assert!(gia_detto(&c, "S", "a", "t").unwrap());
        assert!(!gia_detto(&c, "S", "a", "altro").unwrap());
        assert!(!gia_detto(&c, "S2", "a", "t").unwrap());
    }
}
