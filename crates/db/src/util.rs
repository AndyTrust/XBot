//! Aiuti comuni: errori di SQLite, righe in JSON, aggiornamenti parziali, savepoint.

use rusqlite::types::{Value as ValoreSql, ValueRef};
use rusqlite::{Connection, ErrorCode, Params};
use serde_json::{Map, Value};
use xbot_core::{Codice, Errore, Risposta};

pub(crate) trait RisultatoDb<T> {
    fn db(self) -> Risposta<T>;
}

impl<T> RisultatoDb<T> for rusqlite::Result<T> {
    fn db(self) -> Risposta<T> {
        self.map_err(errore_db)
    }
}

/// Il nome dell'utente scelto a `impianta` (tabella `utente`), «utente» se manca.
pub fn nome_utente(conn: &Connection) -> String {
    conn.query_row("SELECT nome FROM utente WHERE id = 1", [], |r| {
        r.get::<_, String>(0)
    })
    .ok()
    .map(|n| n.trim().to_string())
    .filter(|n| !n.is_empty())
    .unwrap_or_else(|| "utente".into())
}

pub(crate) fn errore_db(e: rusqlite::Error) -> Errore {
    match &e {
        // Un riferimento che non esiste e che i controlli non hanno preso: mai il testo di SQLite.
        rusqlite::Error::SqliteFailure(f, _)
            if f.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY =>
        {
            Errore::nuovo(
                Codice::RiferimentoNonTrovato,
                "un riferimento della richiesta non esiste",
            )
        }
        rusqlite::Error::SqliteFailure(f, _) if f.code == ErrorCode::ConstraintViolation => {
            Errore::non_valida(format!("dato non accettato: {e}"))
        }
        rusqlite::Error::QueryReturnedNoRows => Errore::nuovo(Codice::NonTrovato, "non trovato"),
        _ => Errore::database(e.to_string()),
    }
}

fn valore_json(nome: &str, v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::from(i),
        ValueRef::Real(f) => serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number),
        ValueRef::Text(t) => {
            let s = String::from_utf8_lossy(t).into_owned();
            if nome.ends_with("_json") {
                serde_json::from_str(&s).unwrap_or(Value::String(s))
            } else {
                Value::String(s)
            }
        }
        // I BLOB sono cifrati o binari: non escono mai in JSON.
        ValueRef::Blob(_) => Value::Null,
    }
}

/// Tutte le righe di una query come oggetti JSON (le colonne `_json` già lette).
pub fn tutte_json(conn: &Connection, sql: &str, parametri: impl Params) -> Risposta<Vec<Value>> {
    let mut st = conn.prepare(sql).db()?;
    let nomi: Vec<String> = st.column_names().map(|s| s.to_string()).collect();
    let righe = st
        .query_map(parametri, |r| {
            let mut m = Map::new();
            for (i, n) in nomi.iter().enumerate() {
                m.insert(n.clone(), valore_json(n, r.get_ref(i)?));
            }
            Ok(Value::Object(m))
        })
        .db()?;
    righe.collect::<rusqlite::Result<Vec<_>>>().db()
}

/// La prima riga di una query come oggetto JSON.
pub fn una_json(conn: &Connection, sql: &str, parametri: impl Params) -> Risposta<Option<Value>> {
    Ok(tutte_json(conn, sql, parametri)?.into_iter().next())
}

fn valore_sql(v: &Value) -> ValoreSql {
    match v {
        Value::Null => ValoreSql::Null,
        Value::Bool(b) => ValoreSql::Integer(i64::from(*b)),
        Value::Number(n) => match n.as_i64() {
            Some(i) => ValoreSql::Integer(i),
            None => ValoreSql::Real(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(s) => ValoreSql::Text(s.clone()),
        altro => ValoreSql::Text(altro.to_string()),
    }
}

/// Aggiorna solo le colonne ammesse presenti in `campi`. Le colonne vengono dalla lista
/// `ammessi` (mai dal client), i valori passano come parametri. Restituisce le righe toccate.
pub fn aggiorna_campi(
    conn: &Connection,
    tabella: &str,
    colonna_id: &str,
    id: &str,
    campi: &Map<String, Value>,
    ammessi: &[&str],
    extra: &[(&str, Value)],
) -> Risposta<usize> {
    let mut sets = Vec::new();
    let mut valori = Vec::new();
    for (k, v) in campi {
        let Some(col) = ammessi.iter().find(|a| **a == k.as_str()) else {
            return Err(Errore::non_valida(format!("campo non modificabile: {k}")));
        };
        sets.push(format!("{col} = ?"));
        valori.push(valore_sql(v));
    }
    if sets.is_empty() {
        return Ok(0);
    }
    for (col, v) in extra {
        sets.push(format!("{col} = ?"));
        valori.push(valore_sql(v));
    }
    valori.push(ValoreSql::Text(id.to_string()));
    let sql = format!(
        "UPDATE {tabella} SET {} WHERE {colonna_id} = ?",
        sets.join(", ")
    );
    conn.execute(&sql, rusqlite::params_from_iter(valori)).db()
}

/// Esegue `f` dentro un SAVEPOINT (annidabile): annulla tutto se `f` fallisce.
pub fn in_transazione<T>(conn: &Connection, f: impl FnOnce() -> Risposta<T>) -> Risposta<T> {
    conn.execute_batch("SAVEPOINT xbot").db()?;
    match f() {
        Ok(v) => {
            conn.execute_batch("RELEASE xbot").db()?;
            Ok(v)
        }
        Err(e) => {
            if let Err(e2) = conn.execute_batch("ROLLBACK TO xbot; RELEASE xbot") {
                tracing::error!(errore = %e2, "rollback del savepoint fallito");
            }
            Err(e)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn savepoint_annulla() {
        let db = crate::db_prova();
        db.con(|c| {
            let r: Risposta<()> = in_transazione(c, || {
                c.execute("INSERT INTO sezione_barra (id, nome) VALUES ('x', 'X')", [])
                    .db()?;
                Err(Errore::interno("prova"))
            });
            assert!(r.is_err());
            let n: i64 = c
                .query_row("SELECT COUNT(*) FROM sezione_barra WHERE id='x'", [], |r| {
                    r.get(0)
                })
                .db()?;
            assert_eq!(n, 0);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn campi_non_ammessi_rifiutati() {
        let db = crate::db_prova();
        db.con(|c| {
            let mut m = Map::new();
            m.insert("id".into(), Value::from("y"));
            let r = aggiorna_campi(c, "sezione_barra", "id", "x", &m, &["nome"], &[]);
            assert!(r.is_err());
            Ok(())
        })
        .unwrap();
    }
}
