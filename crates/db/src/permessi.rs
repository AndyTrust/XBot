//! Permessi per azione (concessioni stabili) e regole di revisione.

use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::Value;
use xbot_core::{Codice, Errore, Risposta, nuovo_id, ora_ms};

use crate::util::{RisultatoDb, in_transazione, tutte_json};

/// Regole di revisione al massimo, e caratteri per regola.
pub const MAX_REGOLE: usize = 20;
pub const MAX_TESTO_REGOLA: usize = 1000;

/// Una concessione stabile (`sempre` o `mai`) per un'azione.
#[derive(Debug, Clone, Default)]
pub struct NuovoPermesso {
    pub macchina_id: Option<String>,
    pub assistente_id: Option<String>,
    pub azione: String,
    pub destinatario: Option<String>,
    pub risorsa: Option<String>,
    /// `sempre` | `mai`
    pub concesso: String,
}

pub fn concedi(conn: &Connection, p: &NuovoPermesso) -> Risposta<String> {
    if p.azione.trim().is_empty() {
        return Err(Errore::non_valida("azione obbligatoria"));
    }
    if !matches!(p.concesso.as_str(), "sempre" | "mai") {
        return Err(Errore::non_valida("concesso vuole sempre o mai"));
    }
    let id = nuovo_id();
    conn.execute(
        "INSERT INTO permesso_azione (id, macchina_id, assistente_id, azione, destinatario, risorsa,
           concesso, concesso_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            p.macchina_id,
            p.assistente_id,
            p.azione,
            p.destinatario,
            p.risorsa,
            p.concesso,
            ora_ms()
        ],
    )
    .db()?;
    Ok(id)
}

/// Le concessioni attive, le più recenti prima.
pub fn elenca(conn: &Connection) -> Risposta<Vec<Value>> {
    tutte_json(
        conn,
        "SELECT * FROM permesso_azione WHERE ritirato_ms IS NULL ORDER BY concesso_ms DESC",
        [],
    )
}

/// Ritira una concessione (resta nel database con `ritirato_ms`).
pub fn ritira(conn: &Connection, id: &str) -> Risposta<()> {
    let n = conn
        .execute(
            "UPDATE permesso_azione SET ritirato_ms = ?2 WHERE id = ?1 AND ritirato_ms IS NULL",
            params![id, ora_ms()],
        )
        .db()?;
    if n == 0 {
        return Err(Errore::nuovo(Codice::NonTrovato, "permesso non trovato"));
    }
    Ok(())
}

/// La concessione che vale per un'azione (`sempre` | `mai`), o `None`. Un campo nullo della
/// concessione vale per tutti; vince la più specifica, a pari merito la più recente.
pub fn cerca(
    conn: &Connection,
    azione: &str,
    destinatario: Option<&str>,
    risorsa: Option<&str>,
    assistente_id: Option<&str>,
    macchina_id: Option<&str>,
) -> Risposta<Option<String>> {
    conn.query_row(
        "SELECT concesso FROM permesso_azione
         WHERE ritirato_ms IS NULL AND azione = ?1
           AND (destinatario IS NULL OR destinatario = ?2)
           AND (risorsa IS NULL OR risorsa = ?3)
           AND (assistente_id IS NULL OR assistente_id = ?4)
           AND (macchina_id IS NULL OR macchina_id = ?5)
         ORDER BY (destinatario IS NOT NULL) + (risorsa IS NOT NULL) + (assistente_id IS NOT NULL)
           + (macchina_id IS NOT NULL) DESC, concesso_ms DESC LIMIT 1",
        params![azione, destinatario, risorsa, assistente_id, macchina_id],
        |r| r.get(0),
    )
    .optional()
    .db()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NuovaRegola {
    /// `consenti` | `blocca`
    pub tipo: String,
    pub testo: String,
    #[serde(default = "vero")]
    pub attiva: bool,
    #[serde(default)]
    pub assistente_id: Option<String>,
}

fn vero() -> bool {
    true
}

pub fn regole(conn: &Connection) -> Risposta<Vec<Value>> {
    tutte_json(conn, "SELECT * FROM regola_revisione ORDER BY ordine", [])
}

/// Sostituisce tutte le regole (al massimo 20, ciascuna al massimo 1000 caratteri).
pub fn scrivi_regole(conn: &Connection, regole_nuove: &[NuovaRegola]) -> Risposta<Vec<Value>> {
    if regole_nuove.len() > MAX_REGOLE {
        return Err(Errore::non_valida(format!(
            "al massimo {MAX_REGOLE} regole"
        )));
    }
    for r in regole_nuove {
        if !matches!(r.tipo.as_str(), "consenti" | "blocca") {
            return Err(Errore::non_valida("tipo vuole consenti o blocca"));
        }
        if r.testo.trim().is_empty() {
            return Err(Errore::non_valida("una regola vuota"));
        }
        crate::controlli::lunghezza("testo", Some(&r.testo), MAX_TESTO_REGOLA)?;
    }
    in_transazione(conn, || {
        conn.execute("DELETE FROM regola_revisione", []).db()?;
        let ora = ora_ms();
        for (i, r) in regole_nuove.iter().enumerate() {
            conn.execute(
                "INSERT INTO regola_revisione (id, assistente_id, tipo, testo, ordine, attiva, creata_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    nuovo_id(),
                    r.assistente_id,
                    r.tipo,
                    r.testo.trim(),
                    i64::try_from(i).unwrap_or(0),
                    i64::from(r.attiva),
                    ora
                ],
            )
            .db()?;
        }
        regole(conn)
    })
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn concessioni_e_regole() {
        let db = crate::db_prova();
        db.con(|c| {
            assert!(elenca(c)?.is_empty());
            assert!(regole(c)?.is_empty());
            let generale = concedi(
                c,
                &NuovoPermesso {
                    azione: "run-command".into(),
                    concesso: "sempre".into(),
                    ..Default::default()
                },
            )?;
            concedi(
                c,
                &NuovoPermesso {
                    azione: "run-command".into(),
                    risorsa: Some("rm -rf /".into()),
                    concesso: "mai".into(),
                    ..Default::default()
                },
            )?;
            assert_eq!(
                cerca(c, "run-command", None, Some("ls"), None, None)?.as_deref(),
                Some("sempre")
            );
            assert_eq!(
                cerca(c, "run-command", None, Some("rm -rf /"), None, None)?.as_deref(),
                Some("mai")
            );
            assert_eq!(cerca(c, "write-file", None, None, None, None)?, None);
            ritira(c, &generale)?;
            assert!(ritira(c, &generale).is_err());
            assert_eq!(elenca(c)?.len(), 1);
            let r = vec![NuovaRegola {
                tipo: "blocca".into(),
                testo: "niente sudo".into(),
                attiva: true,
                assistente_id: None,
            }];
            assert_eq!(scrivi_regole(c, &r)?.len(), 1);
            assert!(scrivi_regole(c, &vec![r[0].clone(); 21]).is_err());
            Ok(())
        })
        .unwrap();
    }
}
