//! Fase 2.K: dove gira un collegamento. Uno con `macchina_id` entra solo nei turni di quel
//! computer; uno senza è del server se è stdio (il comando sta lì), di tutti se è http.

use rusqlite::{Connection, params};
use xbot_core::Risposta;

use super::CollegamentoVivo;
use super::vivi::vivo;
use crate::cassaforte::Chiave;
use crate::util::RisultatoDb;

/// Il collegamento `c` entra in un turno su `?2` (`?3` = 1 se è la macchina del server).
/// `IFNULL` perché `NOT (NULL OR falso)` è NULL in SQL e perderebbe le righe.
const QUI: &str = "(IFNULL(c.macchina_id, '') = ?2 OR (c.macchina_id IS NULL AND (c.trasporto = 'http' OR ?3 = 1)))";

fn ids(conn: &Connection, sql: &str, aid: &str, qui: &str, server: bool) -> Risposta<Vec<String>> {
    let mut st = conn.prepare(sql).db()?;
    st.query_map(params![aid, qui, i64::from(server)], |r| r.get(0))
        .db()?
        .collect::<rusqlite::Result<_>>()
        .db()
}

fn accesi(dentro: bool) -> String {
    format!(
        "SELECT c.id FROM collegamento c WHERE c.abilitato = 1 AND {}{QUI} AND {}
         ORDER BY c.nome, c.id",
        if dentro { "" } else { "NOT " },
        super::NON_ESCLUSO,
    )
}

/// Come [`super::per_assistente`], ma solo i collegamenti che girano sulla macchina `qui`
/// (`server`: è la macchina del server).
pub fn per_assistente_su(
    conn: &Connection,
    chiave: Option<&Chiave>,
    assistente_id: &str,
    qui: &str,
    server: bool,
) -> Risposta<Vec<CollegamentoVivo>> {
    let mut v = Vec::new();
    for id in ids(conn, &accesi(true), assistente_id, qui, server)? {
        match vivo(conn, chiave, &id) {
            Ok(c) if c.pronto() => v.push(c),
            Ok(_) => {
                tracing::info!(collegamento = %id, "collegamento senza accesso OAuth valido: fuori dal turno")
            }
            Err(e) => {
                tracing::warn!(collegamento = %id, codice = e.codice.come_testo(), "collegamento saltato nel turno")
            }
        }
    }
    Ok(v)
}

/// I collegamenti accesi dell'assistente che girano altrove: (nome, nome della macchina; `None`
/// = il server).
pub fn fuori_dal_turno(
    conn: &Connection,
    assistente_id: &str,
    qui: &str,
    server: bool,
) -> Risposta<Vec<(String, Option<String>)>> {
    let mut v = Vec::new();
    for id in ids(conn, &accesi(false), assistente_id, qui, server)? {
        let r = conn
            .query_row(
                "SELECT c.nome, m.nome FROM collegamento c LEFT JOIN macchina m ON m.id = c.macchina_id
                 WHERE c.id = ?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .db()?;
        v.push(r);
    }
    Ok(v)
}
