//! Stato di lettura per assistente: letto fino a, al lavoro, in attesa dell'utente.

use rusqlite::{Connection, params};
use xbot_core::{Risposta, ora_ms};

use crate::util::RisultatoDb;

/// Segna letto fino a `fino_seq` (non torna mai indietro) e azzera l'attesa.
pub fn segna_letto(conn: &Connection, assistente_id: &str, fino_seq: i64) -> Risposta<()> {
    conn.execute(
        "INSERT INTO stato_lettura (assistente_id, letto_fino_seq, ultima_vista_ms)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(assistente_id) DO UPDATE SET
           letto_fino_seq = MAX(letto_fino_seq, excluded.letto_fino_seq),
           in_attesa_utente = 0, ultima_vista_ms = excluded.ultima_vista_ms",
        params![assistente_id, fino_seq, ora_ms()],
    )
    .db()?;
    Ok(())
}

/// Aggiorna i due segnali della barra.
pub fn imposta_lavoro(
    conn: &Connection,
    assistente_id: &str,
    al_lavoro: bool,
    in_attesa_utente: bool,
) -> Risposta<()> {
    conn.execute(
        "INSERT INTO stato_lettura (assistente_id, al_lavoro, in_attesa_utente) VALUES (?1, ?2, ?3)
         ON CONFLICT(assistente_id) DO UPDATE SET
           al_lavoro = excluded.al_lavoro, in_attesa_utente = excluded.in_attesa_utente",
        params![
            assistente_id,
            i64::from(al_lavoro),
            i64::from(in_attesa_utente)
        ],
    )
    .db()?;
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::assistenti::{self, NuovoAssistente};

    #[test]
    fn letto_non_torna_indietro() {
        let db = crate::db_prova();
        db.con(|c| {
            let a = assistenti::crea(c, &NuovoAssistente { nome: "J".into(), ..Default::default() })?;
            let id = a["id"].as_str().unwrap();
            imposta_lavoro(c, id, true, true)?;
            segna_letto(c, id, 5)?;
            segna_letto(c, id, 3)?;
            let (letto, attesa, lavoro): (i64, i64, i64) = c
                .query_row(
                    "SELECT letto_fino_seq, in_attesa_utente, al_lavoro FROM stato_lettura WHERE assistente_id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .db()?;
            assert_eq!((letto, attesa, lavoro), (5, 0, 1));
            Ok(())
        })
        .unwrap();
    }
}
