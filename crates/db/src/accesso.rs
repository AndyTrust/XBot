//! L'utente unico e le sessioni di accesso web (si salva l'hash del token, mai il token).

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use xbot_core::{Ms, Risposta, ora_ms};

use crate::util::RisultatoDb;

/// Crea o aggiorna l'utente (id = 1).
pub fn imposta_utente(conn: &Connection, nome: &str, totp_cifrato: Option<&[u8]>) -> Risposta<()> {
    conn.execute(
        "INSERT INTO utente (id, nome, totp_cifrato, creato_ms) VALUES (1, ?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET nome = excluded.nome, totp_cifrato = excluded.totp_cifrato",
        params![nome, totp_cifrato, ora_ms()],
    )
    .db()?;
    Ok(())
}

/// Nome e segreto TOTP cifrato, se l'utente esiste.
pub fn utente(conn: &Connection) -> Risposta<Option<(String, Option<Vec<u8>>)>> {
    conn.query_row(
        "SELECT nome, totp_cifrato FROM utente WHERE id = 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .db()
}

#[derive(Debug, Clone, Serialize)]
pub struct AccessoWeb {
    #[serde(skip)]
    pub id: String,
    pub dispositivo: String,
    pub creato_ms: Ms,
    pub scade_ms: Ms,
}

pub fn crea(conn: &Connection, id_hash: &str, dispositivo: &str, scade_ms: Ms) -> Risposta<()> {
    conn.execute(
        "INSERT INTO accesso_web (id, dispositivo, creato_ms, ultimo_uso_ms, scade_ms)
         VALUES (?1, ?2, ?3, ?3, ?4)",
        params![id_hash, dispositivo, ora_ms(), scade_ms],
    )
    .db()?;
    Ok(())
}

/// L'accesso valido (non scaduto, non revocato); ne aggiorna l'ultimo uso.
pub fn valido(conn: &Connection, id_hash: &str) -> Risposta<Option<AccessoWeb>> {
    let ora = ora_ms();
    let a = conn
        .query_row(
            "SELECT id, dispositivo, creato_ms, scade_ms FROM accesso_web
             WHERE id = ?1 AND revocato_ms IS NULL AND scade_ms > ?2",
            params![id_hash, ora],
            |r| {
                Ok(AccessoWeb {
                    id: r.get(0)?,
                    dispositivo: r.get(1)?,
                    creato_ms: r.get(2)?,
                    scade_ms: r.get(3)?,
                })
            },
        )
        .optional()
        .db()?;
    if a.is_some() {
        conn.execute(
            "UPDATE accesso_web SET ultimo_uso_ms = ?2 WHERE id = ?1",
            params![id_hash, ora],
        )
        .db()?;
    }
    Ok(a)
}

pub fn revoca(conn: &Connection, id_hash: &str) -> Risposta<()> {
    conn.execute(
        "UPDATE accesso_web SET revocato_ms = ?2 WHERE id = ?1 AND revocato_ms IS NULL",
        params![id_hash, ora_ms()],
    )
    .db()?;
    Ok(())
}

// --- accesso fidato (migrazione 008) ---

/// Un dispositivo fidato come lo vede l'interfaccia (mai l'hash).
#[derive(Debug, Clone, Serialize)]
pub struct DispositivoFidato {
    pub id: String,
    pub nome: String,
    pub creato_ms: Ms,
    pub ultimo_uso_ms: Ms,
    pub scade_ms: Ms,
}

pub fn fidato_crea(
    conn: &Connection,
    id: &str,
    nome: &str,
    hash: &str,
    scade_ms: Ms,
    agente: Option<&str>,
) -> Risposta<()> {
    conn.execute(
        "INSERT INTO dispositivo_fidato (id, nome, hash, creato_ms, ultimo_uso_ms, scade_ms, agente)
         VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6)",
        params![id, nome, hash, ora_ms(), scade_ms, agente],
    )
    .db()?;
    Ok(())
}

/// L'id del dispositivo fidato valido (non scaduto, non revocato) con questo hash.
pub fn fidato_id(conn: &Connection, hash: &str) -> Risposta<Option<String>> {
    conn.query_row(
        "SELECT id FROM dispositivo_fidato
         WHERE hash = ?1 AND revocato_ms IS NULL AND scade_ms > ?2",
        params![hash, ora_ms()],
        |r| r.get(0),
    )
    .optional()
    .db()
}

/// Come `fidato_id`, e ne aggiorna l'ultimo uso e la scadenza (`nuova_scade_ms`): chi usa il
/// dispositivo non deve rifare il codice finché non lo revoca.
pub fn fidato_usa(conn: &Connection, hash: &str, nuova_scade_ms: Ms) -> Risposta<Option<String>> {
    let id = fidato_id(conn, hash)?;
    if let Some(id) = &id {
        conn.execute(
            "UPDATE dispositivo_fidato SET ultimo_uso_ms = ?2, scade_ms = MAX(scade_ms, ?3) WHERE id = ?1",
            params![id, ora_ms(), nuova_scade_ms],
        )
        .db()?;
    }
    Ok(id)
}

/// I dispositivi fidati validi, dal più recente.
pub fn fidati(conn: &Connection) -> Risposta<Vec<DispositivoFidato>> {
    let mut st = conn
        .prepare(
            "SELECT id, nome, creato_ms, ultimo_uso_ms, scade_ms FROM dispositivo_fidato
             WHERE revocato_ms IS NULL AND scade_ms > ?1 ORDER BY creato_ms DESC, id DESC",
        )
        .db()?;
    let righe = st
        .query_map([ora_ms()], |r| {
            Ok(DispositivoFidato {
                id: r.get(0)?,
                nome: r.get(1)?,
                creato_ms: r.get(2)?,
                ultimo_uso_ms: r.get(3)?,
                scade_ms: r.get(4)?,
            })
        })
        .db()?;
    righe.collect::<Result<Vec<_>, _>>().db()
}

/// Revoca un dispositivo fidato valido; `false` se non c'è (o è già revocato o scaduto).
pub fn fidato_revoca(conn: &Connection, id: &str) -> Risposta<bool> {
    let n = conn
        .execute(
            "UPDATE dispositivo_fidato SET revocato_ms = ?2
             WHERE id = ?1 AND revocato_ms IS NULL AND scade_ms > ?2",
            params![id, ora_ms()],
        )
        .db()?;
    Ok(n > 0)
}

/// Revoca tutti i dispositivi fidati validi; dà quanti.
pub fn fidati_revoca_tutti(conn: &Connection) -> Risposta<usize> {
    conn.execute(
        "UPDATE dispositivo_fidato SET revocato_ms = ?1
         WHERE revocato_ms IS NULL AND scade_ms > ?1",
        params![ora_ms()],
    )
    .db()
}
// --- fine accesso fidato ---

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn utente_e_accessi() {
        let db = crate::db_prova();
        db.con(|c| {
            assert!(utente(c)?.is_none());
            imposta_utente(c, "Mario", Some(&[1, 2, 3]))?;
            imposta_utente(c, "Mario", None)?;
            assert_eq!(utente(c)?.unwrap(), ("Mario".to_string(), None));
            crea(c, "h1", "Mac", ora_ms() + 60_000)?;
            crea(c, "h2", "S24", ora_ms() - 1)?;
            assert!(valido(c, "h1")?.is_some());
            assert!(valido(c, "h2")?.is_none(), "scaduto");
            revoca(c, "h1")?;
            assert!(valido(c, "h1")?.is_none(), "revocato");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn dispositivi_fidati() {
        let db = crate::db_prova();
        db.con(|c| {
            fidato_crea(c, "A", "Mac", "ha", ora_ms() + 60_000, Some("ua"))?;
            fidato_crea(c, "B", "S24", "hb", ora_ms() + 60_000, None)?;
            fidato_crea(c, "C", "vecchio", "hc", ora_ms() - 1, None)?;
            assert_eq!(fidato_usa(c, "ha", i64::MAX / 2)?.as_deref(), Some("A"));
            assert!(fidato_id(c, "hc")?.is_none(), "scaduto");
            assert_eq!(fidati(c)?.len(), 2);
            assert!(fidato_revoca(c, "A")?);
            assert!(!fidato_revoca(c, "A")?, "già revocato");
            assert!(fidato_id(c, "ha")?.is_none());
            assert_eq!(fidati_revoca_tutti(c)?, 1);
            assert!(fidati(c)?.is_empty());
            Ok(())
        })
        .unwrap();
    }
}
