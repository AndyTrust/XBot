//! Righe di trascrizione: `seq` fisso per riga, `aggiornata_seq` crescente a ogni scrittura.

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use serde_json::Value;
use xbot_core::{Codice, Errore, GenereRiga, Id, Ms, Risposta, Ruolo, nuovo_id, ora_ms};

use crate::util::{RisultatoDb, in_transazione};

#[derive(Debug, Clone, Serialize)]
pub struct Riga {
    pub sessione_id: Id,
    pub seq: i64,
    pub id: Id,
    pub genere: String,
    pub ruolo: Option<String>,
    pub corpo: Value,
    pub blob_hash: Option<String>,
    pub aggiornata_seq: i64,
    pub creata_ms: Ms,
    pub aggiornata_ms: Ms,
}

const COLONNE: &str = "sessione_id, seq, id, genere, ruolo, corpo_json, blob_hash, aggiornata_seq, creata_ms, aggiornata_ms";

fn da_riga(r: &Row<'_>) -> rusqlite::Result<Riga> {
    let corpo: Option<String> = r.get(5)?;
    Ok(Riga {
        sessione_id: r.get(0)?,
        seq: r.get(1)?,
        id: r.get(2)?,
        genere: r.get(3)?,
        ruolo: r.get(4)?,
        corpo: corpo
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null),
        blob_hash: r.get(6)?,
        aggiornata_seq: r.get(7)?,
        creata_ms: r.get(8)?,
        aggiornata_ms: r.get(9)?,
    })
}

fn leggi_per_id(conn: &Connection, id: &str) -> Risposta<Riga> {
    conn.query_row(
        &format!("SELECT {COLONNE} FROM riga WHERE id = ?1"),
        [id],
        da_riga,
    )
    .optional()
    .db()?
    .ok_or_else(|| Errore::nuovo(Codice::NonTrovato, "riga non trovata"))
}

/// Una riga per id (`non_trovato` se manca).
pub fn leggi(conn: &Connection, id: &str) -> Risposta<Riga> {
    leggi_per_id(conn, id)
}

/// Aggiunge una riga: `ultimo_seq+1` e `ultimo_aggiornata_seq+1` nella stessa transazione.
pub fn aggiungi(
    conn: &Connection,
    sessione_id: &str,
    genere: GenereRiga,
    ruolo: Option<Ruolo>,
    corpo: &Value,
) -> Risposta<Riga> {
    let ora = ora_ms();
    in_transazione(conn, || {
        let contatori: Option<(i64, i64)> = conn
            .query_row(
                "UPDATE sessione SET ultimo_seq = ultimo_seq + 1,
                   ultimo_aggiornata_seq = ultimo_aggiornata_seq + 1, aggiornata_ms = ?2
                 WHERE id = ?1 RETURNING ultimo_seq, ultimo_aggiornata_seq",
                params![sessione_id, ora],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .db()?;
        let (seq, agg) = contatori
            .ok_or_else(|| Errore::nuovo(Codice::SessioneNonTrovata, "sessione non trovata"))?;
        let id = nuovo_id();
        conn.execute(
            "INSERT INTO riga (sessione_id, seq, id, genere, ruolo, corpo_json, aggiornata_seq,
               creata_ms, aggiornata_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![
                sessione_id,
                seq,
                id,
                genere.come_testo(),
                ruolo.map(Ruolo::come_testo),
                corpo.to_string(),
                agg,
                ora
            ],
        )
        .db()?;
        leggi_per_id(conn, &id)
    })
}

/// Riscrive il corpo di una riga: stesso `seq`, `aggiornata_seq = ultimo_aggiornata_seq+1`.
pub fn riscrivi(conn: &Connection, riga_id: &str, corpo: &Value) -> Risposta<Riga> {
    let ora = ora_ms();
    in_transazione(conn, || {
        let vecchia = leggi_per_id(conn, riga_id)?;
        let agg: i64 = conn
            .query_row(
                "UPDATE sessione SET ultimo_aggiornata_seq = ultimo_aggiornata_seq + 1,
                   aggiornata_ms = ?2 WHERE id = ?1 RETURNING ultimo_aggiornata_seq",
                params![vecchia.sessione_id, ora],
                |r| r.get(0),
            )
            .db()?;
        conn.execute(
            "UPDATE riga SET corpo_json = ?2, aggiornata_seq = ?3, aggiornata_ms = ?4 WHERE id = ?1",
            params![riga_id, corpo.to_string(), agg, ora],
        )
        .db()?;
        leggi_per_id(conn, riga_id)
    })
}

/// Una pagina di righe dopo un cursore.
#[derive(Debug, Clone, Serialize)]
pub struct Pagina {
    pub generazione: i64,
    pub ultimo_aggiornata_seq: i64,
    pub righe: Vec<Riga>,
    /// Vero se ci sono altre righe oltre il limite.
    pub altre: bool,
}

/// Le righe (non soppresse) con `aggiornata_seq > dopo`, in ordine di aggiornamento.
/// Se `generazione` è data e non è quella corrente: `cursore_vecchio`.
pub fn dopo(
    conn: &Connection,
    sessione_id: &str,
    generazione: Option<i64>,
    dopo_aggiornata_seq: i64,
    limite: i64,
) -> Risposta<Pagina> {
    let stato: Option<(i64, i64)> = conn
        .query_row(
            "SELECT generazione, ultimo_aggiornata_seq FROM sessione WHERE id = ?1",
            [sessione_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .db()?;
    let (gen_corrente, ultimo) =
        stato.ok_or_else(|| Errore::nuovo(Codice::SessioneNonTrovata, "sessione non trovata"))?;
    if let Some(g) = generazione
        && g != gen_corrente
    {
        return Err(Errore::nuovo(
            Codice::CursoreVecchio,
            format!("generazione {g} superata: ora è {gen_corrente}"),
        ));
    }
    let limite = limite.clamp(1, 1000);
    let mut st = conn
        .prepare(&format!(
            "SELECT {COLONNE} FROM riga WHERE sessione_id = ?1 AND soppressa = 0
               AND aggiornata_seq > ?2 ORDER BY aggiornata_seq LIMIT ?3"
        ))
        .db()?;
    let mut righe = st
        .query_map(
            params![sessione_id, dopo_aggiornata_seq, limite + 1],
            da_riga,
        )
        .db()?
        .collect::<rusqlite::Result<Vec<_>>>()
        .db()?;
    let altre = righe.len() as i64 > limite;
    righe.truncate(usize::try_from(limite).unwrap_or(1000));
    Ok(Pagina {
        generazione: gen_corrente,
        ultimo_aggiornata_seq: ultimo,
        righe,
        altre,
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::sessioni;
    use serde_json::json;

    #[test]
    fn aggiungi_e_riscrivi_seguono_i_contatori() {
        let db = crate::db_prova();
        db.con(|c| {
            let s = sessioni::test::sessione_prova(c);
            let a = aggiungi(
                c,
                &s.id,
                GenereRiga::Messaggio,
                Some(Ruolo::Utente),
                &json!({"testo": "ciao"}),
            )?;
            assert_eq!((a.seq, a.aggiornata_seq), (1, 1));
            let b = aggiungi(
                c,
                &s.id,
                GenereRiga::Messaggio,
                Some(Ruolo::Assistente),
                &json!({"testo": "o"}),
            )?;
            assert_eq!((b.seq, b.aggiornata_seq), (2, 2));
            let b2 = riscrivi(c, &b.id, &json!({"testo": "oai"}))?;
            assert_eq!((b2.seq, b2.aggiornata_seq), (2, 3));
            let s2 = sessioni::leggi(c, &s.id)?;
            assert_eq!((s2.ultimo_seq, s2.ultimo_aggiornata_seq), (2, 3));

            let p = dopo(c, &s.id, Some(1), 1, 100)?;
            assert_eq!(p.righe.len(), 1);
            assert_eq!(p.righe[0].corpo["testo"], "oai");
            assert!(!p.altre);
            let p = dopo(c, &s.id, None, 0, 1)?;
            assert!(p.altre);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn generazione_vecchia_e_cursore_vecchio() {
        let db = crate::db_prova();
        db.con(|c| {
            let s = sessioni::test::sessione_prova(c);
            aggiungi(
                c,
                &s.id,
                GenereRiga::Avviso,
                Some(Ruolo::Sistema),
                &json!({}),
            )?;
            let g = sessioni::azzera(c, &s.id)?;
            let e = dopo(c, &s.id, Some(1), 0, 10).unwrap_err();
            assert_eq!(e.codice, Codice::CursoreVecchio);
            let p = dopo(c, &s.id, Some(g), 0, 10)?;
            assert!(
                p.righe.is_empty(),
                "le righe della generazione vecchia non si vedono"
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn sessione_mancante() {
        let db = crate::db_prova();
        db.con(|c| {
            let e = aggiungi(c, "x", GenereRiga::Evento, None, &json!({})).unwrap_err();
            assert_eq!(e.codice, Codice::SessioneNonTrovata);
            Ok(())
        })
        .unwrap();
    }
}
