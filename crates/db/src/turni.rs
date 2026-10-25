//! Turni: un giro del provider per un invio, con token, costo e durata.

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use xbot_core::{Codice, Errore, Id, Ms, Risposta, StatoTurno, nuovo_id, ora_ms};

use crate::util::RisultatoDb;

#[derive(Debug, Clone, Serialize)]
pub struct Turno {
    pub id: Id,
    pub sessione_id: Id,
    pub invio_id: Option<String>,
    pub provider_id: String,
    pub modello_id: Option<String>,
    pub livello: Option<String>,
    pub stato: String,
    pub processo_pid: Option<i64>,
    pub iniziato_ms: Ms,
    pub finito_ms: Option<Ms>,
    pub durata_ms: Option<Ms>,
    pub token_ingresso: Option<i64>,
    pub token_uscita: Option<i64>,
    pub token_cache: Option<i64>,
    pub costo_usd: Option<f64>,
    pub errore_codice: Option<String>,
    pub errore_testo: Option<String>,
}

const COLONNE: &str = "id, sessione_id, invio_id, provider_id, modello_id, livello, stato, \
    processo_pid, iniziato_ms, finito_ms, durata_ms, token_ingresso, token_uscita, token_cache, \
    costo_usd, errore_codice, errore_testo";

fn da_riga(r: &Row<'_>) -> rusqlite::Result<Turno> {
    Ok(Turno {
        id: r.get(0)?,
        sessione_id: r.get(1)?,
        invio_id: r.get(2)?,
        provider_id: r.get(3)?,
        modello_id: r.get(4)?,
        livello: r.get(5)?,
        stato: r.get(6)?,
        processo_pid: r.get(7)?,
        iniziato_ms: r.get(8)?,
        finito_ms: r.get(9)?,
        durata_ms: r.get(10)?,
        token_ingresso: r.get(11)?,
        token_uscita: r.get(12)?,
        token_cache: r.get(13)?,
        costo_usd: r.get(14)?,
        errore_codice: r.get(15)?,
        errore_testo: r.get(16)?,
    })
}

#[derive(Debug, Clone)]
pub struct NuovoTurno {
    pub sessione_id: Id,
    pub invio_id: Option<Id>,
    pub provider_id: String,
    pub modello_id: Option<String>,
    pub livello: Option<String>,
    pub macchina_id: Option<String>,
}

pub fn crea(conn: &Connection, n: &NuovoTurno) -> Risposta<Turno> {
    let id = nuovo_id();
    conn.execute(
        "INSERT INTO turno (id, sessione_id, invio_id, provider_id, modello_id, livello, macchina_id,
           iniziato_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            n.sessione_id,
            n.invio_id,
            n.provider_id,
            n.modello_id,
            n.livello,
            n.macchina_id,
            ora_ms()
        ],
    )
    .db()?;
    leggi(conn, &id)
}

pub fn leggi(conn: &Connection, id: &str) -> Risposta<Turno> {
    conn.query_row(
        &format!("SELECT {COLONNE} FROM turno WHERE id = ?1"),
        [id],
        da_riga,
    )
    .optional()
    .db()?
    .ok_or_else(|| Errore::nuovo(Codice::TurnoNonTrovato, "turno non trovato"))
}

pub fn imposta_pid(conn: &Connection, id: &str, pid: u32) -> Risposta<()> {
    conn.execute(
        "UPDATE turno SET processo_pid = ?2 WHERE id = ?1",
        params![id, i64::from(pid)],
    )
    .db()?;
    Ok(())
}

/// Token e costo alla chiusura.
#[derive(Debug, Clone, Default)]
pub struct Conto {
    pub token_ingresso: Option<i64>,
    pub token_uscita: Option<i64>,
    pub token_cache: Option<i64>,
    pub costo_usd: Option<f64>,
}

/// Chiude un turno ancora `in_corso`, scrive `finito_ms` e `durata_ms`.
/// Restituisce falso se il turno era già chiuso (es. interrotto prima).
pub fn chiudi(
    conn: &Connection,
    id: &str,
    stato: StatoTurno,
    conto: &Conto,
    errore: Option<(&str, &str)>,
) -> Risposta<bool> {
    let ora = ora_ms();
    let n = conn
        .execute(
            "UPDATE turno SET stato = ?2, finito_ms = ?3, durata_ms = ?3 - iniziato_ms,
               token_ingresso = ?4, token_uscita = ?5, token_cache = ?6, costo_usd = ?7,
               errore_codice = ?8, errore_testo = ?9
             WHERE id = ?1 AND stato = 'in_corso'",
            params![
                id,
                stato.come_testo(),
                ora,
                conto.token_ingresso,
                conto.token_uscita,
                conto.token_cache,
                conto.costo_usd,
                errore.map(|e| e.0),
                errore.map(|e| e.1),
            ],
        )
        .db()?;
    Ok(n == 1)
}

/// I turni rimasti `in_corso` da un avvio precedente diventano `interrotto`.
pub fn chiudi_orfani(conn: &Connection) -> Risposta<usize> {
    let ora = ora_ms();
    conn.execute(
        "UPDATE turno SET stato = 'interrotto', finito_ms = ?1, durata_ms = ?1 - iniziato_ms,
           errore_codice = 'motore_riavviato' WHERE stato = 'in_corso'",
        [ora],
    )
    .db()
}

/// Il turno nato da un invio, se c'è.
pub fn per_invio(conn: &Connection, invio_id: &str) -> Risposta<Option<Turno>> {
    conn.query_row(
        &format!("SELECT {COLONNE} FROM turno WHERE invio_id = ?1 ORDER BY iniziato_ms LIMIT 1"),
        [invio_id],
        da_riga,
    )
    .optional()
    .db()
}

/// L'ultimo turno di una sessione, se c'è.
pub fn ultimo_della_sessione(conn: &Connection, sessione_id: &str) -> Risposta<Option<Turno>> {
    conn.query_row(
        &format!(
            "SELECT {COLONNE} FROM turno WHERE sessione_id = ?1 ORDER BY iniziato_ms DESC, id DESC LIMIT 1"
        ),
        [sessione_id],
        da_riga,
    )
    .optional()
    .db()
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{provider, sessioni};

    #[test]
    fn chiudi_scrive_durata_una_volta_sola() {
        let db = crate::db_prova();
        db.con(|c| {
            provider::registra(c, "finto", "Finto", "interno")?;
            let s = sessioni::test::sessione_prova(c);
            let t = crea(
                c,
                &NuovoTurno {
                    sessione_id: s.id.clone(),
                    invio_id: None,
                    provider_id: "finto".into(),
                    modello_id: None,
                    livello: None,
                    macchina_id: None,
                },
            )?;
            imposta_pid(c, &t.id, 42)?;
            let conto = Conto {
                token_ingresso: Some(3),
                token_uscita: Some(5),
                ..Default::default()
            };
            assert!(chiudi(c, &t.id, StatoTurno::Riuscito, &conto, None)?);
            assert!(!chiudi(c, &t.id, StatoTurno::Fallito, &conto, None)?);
            let t2 = leggi(c, &t.id)?;
            assert_eq!(t2.stato, "riuscito");
            assert!(t2.durata_ms.is_some());
            assert_eq!(t2.processo_pid, Some(42));
            assert_eq!(ultimo_della_sessione(c, &s.id)?.map(|t| t.id), Some(t.id));
            assert_eq!(chiudi_orfani(c)?, 0);
            assert!(per_invio(c, "nessuno")?.is_none());
            Ok(())
        })
        .unwrap();
    }
}
