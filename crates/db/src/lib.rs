//! Il database di XBot: SQLite con lo schema v1 più le migrazioni, una funzione per operazione.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod accesso;
pub mod allegati;
pub mod assistenti;
pub mod barra;
// --- S2 ---
pub mod canali;
// --- fine S2 ---
pub mod cassaforte;
pub mod cassaforte_csv;
pub mod cassaforte_generi;
pub mod cassaforte_voci;
pub mod collegamenti;
pub mod controlli;
pub mod eventi;
pub mod figlie; // sessioni figlie (migrazione 012)
pub mod impostazioni;
pub mod invii;
pub mod lavagna; // sessioni annidate e lavagna comune (migrazione 012)
pub mod lavori; // S-B (2.E)
pub mod lettura;
pub mod macchine;
pub mod notifiche;
pub mod oauth; // 2.I
pub mod permessi;
pub mod provider;
pub mod richieste;
pub mod righe;
pub mod routine;
pub mod sessioni;
pub mod sezioni;
// --- 2.H ---
pub mod computer;
// --- fine 2.H ---
pub mod turni;
mod util;
pub mod utilizzo;

use std::sync::{Arc, Mutex};

pub use rusqlite::Connection as Connessione;
pub use util::{aggiorna_campi, in_transazione, nome_utente, tutte_json, una_json};
use xbot_core::{Errore, Risposta};

/// Lo schema v1, applicato così com'è: è la migrazione 001.
pub const SCHEMA: &str = include_str!("../../../schema.sql");

/// Le migrazioni dopo la 001, in ordine: (versione, SQL). Ognuna in una transazione.
pub const MIGRAZIONI: &[(u32, &str)] = &[
    (2, include_str!("../migrazioni/002.sql")),
    (3, include_str!("../migrazioni/003.sql")),
    (4, include_str!("../migrazioni/004.sql")),
    (5, include_str!("../migrazioni/005.sql")),
    (6, include_str!("../migrazioni/006.sql")),
    (7, include_str!("../migrazioni/007.sql")),
    (8, include_str!("../migrazioni/008.sql")),
    (9, include_str!("../migrazioni/009.sql")),
    (10, include_str!("../migrazioni/010.sql")),
    (11, include_str!("../migrazioni/011.sql")),
    (12, include_str!("../migrazioni/012.sql")),
    (13, include_str!("../migrazioni/013.sql")),
];

/// La versione a cui `apri` porta ogni database.
pub const VERSIONE_SCHEMA: u32 = 13;

/// Il database: una connessione protetta da un `Mutex` (prima versione).
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connessione>>,
}

impl Db {
    /// Apre (o crea) il database e applica lo schema se manca. `":memory:"` per i test.
    pub fn apri(percorso: &str) -> Risposta<Db> {
        let conn = Connessione::open(percorso).map_err(util::errore_db)?;
        prepara(&conn)?;
        Ok(Db {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Esegue `f` con la connessione, nel thread corrente.
    pub fn con<T>(&self, f: impl FnOnce(&Connessione) -> Risposta<T>) -> Risposta<T> {
        let guardia = self
            .conn
            .lock()
            .map_err(|_| Errore::interno("connessione del database avvelenata"))?;
        f(&guardia)
    }

    /// Esegue `f` in un thread di blocco di tokio (per il server).
    pub async fn esegui<T, F>(&self, f: F) -> Risposta<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connessione) -> Risposta<T> + Send + 'static,
    {
        let db = self.clone();
        tokio::task::spawn_blocking(move || db.con(f))
            .await
            .map_err(|e| Errore::interno(format!("compito del database interrotto: {e}")))?
    }
}

/// Pragmi di ogni connessione e schema alla prima apertura.
fn prepara(conn: &Connessione) -> Risposta<()> {
    use util::RisultatoDb;
    conn.pragma_update(None, "foreign_keys", "ON").db()?;
    conn.pragma_update(None, "busy_timeout", 5000).db()?;
    let _modo: String = conn
        .query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))
        .db()?;
    let esiste: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='schema_meta')",
            [],
            |r| r.get(0),
        )
        .db()?;
    if !esiste {
        conn.execute_batch(SCHEMA).db()?;
        tracing::info!("schema v1 applicato");
    }
    migra(conn).map(|_| ())
}

/// Porta il database dalla versione scritta in `schema_meta` all'ultima.
pub fn migra(conn: &Connessione) -> Risposta<u32> {
    let attuale: u32 = versione_schema(conn)?
        .parse()
        .map_err(|_| Errore::database("schema_version non numerica"))?;
    let mut v = attuale;
    for (numero, sql) in MIGRAZIONI.iter().filter(|(n, _)| *n > attuale) {
        in_transazione(conn, || {
            use util::RisultatoDb;
            conn.execute_batch(sql).db()?;
            conn.execute(
                "UPDATE schema_meta SET valore = ?1 WHERE chiave = 'schema_version'",
                [numero.to_string()],
            )
            .db()?;
            Ok(())
        })?;
        tracing::info!(da = v, a = numero, "migrazione applicata");
        v = *numero;
    }
    Ok(v)
}

/// La versione dello schema scritta in `schema_meta`.
pub fn versione_schema(conn: &Connessione) -> Risposta<String> {
    use util::RisultatoDb;
    conn.query_row(
        "SELECT valore FROM schema_meta WHERE chiave = 'schema_version'",
        [],
        |r| r.get(0),
    )
    .db()
}

#[cfg(test)]
pub(crate) fn db_prova() -> Db {
    Db::apri(":memory:").unwrap()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn apre_e_applica_lo_schema_una_volta() {
        let db = db_prova();
        let v = db.con(versione_schema).unwrap();
        assert_eq!(v, VERSIONE_SCHEMA.to_string());
        db.con(prepara).unwrap();
    }

    #[test]
    fn un_database_alla_1_arriva_all_ultima() {
        let conn = Connessione::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        assert_eq!(versione_schema(&conn).unwrap(), "1");
        let vecchia: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'v_barra_laterale'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(vecchia.contains("letto_fino_seq"));
        assert_eq!(migra(&conn).unwrap(), VERSIONE_SCHEMA);
        assert_eq!(versione_schema(&conn).unwrap(), VERSIONE_SCHEMA.to_string());
        // 003: la colonna del costo si chiama costo_usd.
        conn.execute("UPDATE turno SET costo_usd = 0 WHERE 0", [])
            .unwrap();
        let nuova: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name = 'v_barra_laterale'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(nuova.contains("ultima_vista_ms") && nuova.contains("soppressa"));
        // Una seconda volta non fa nulla.
        assert_eq!(migra(&conn).unwrap(), VERSIONE_SCHEMA);
    }

    #[test]
    fn migrazione_013_senza_modifiche() {
        let conn = Connessione::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO assistente (id, nome, avatar_forma, creato_ms, aggiornato_ms)
             VALUES ('a1', 'a1', 'personaggio:maggiordomo-maranza', 0, 0)",
            [],
        )
        .unwrap();
        assert_eq!(migra(&conn).unwrap(), 13);
        let forma: String = conn
            .query_row("SELECT avatar_forma FROM assistente", [], |r| r.get(0))
            .unwrap();
        assert_eq!(forma, "personaggio:maggiordomo-maranza");
    }

    #[test]
    fn file_su_disco_si_riapre() {
        let dir = std::env::temp_dir().join(format!("xbot-db-{}", xbot_core::nuovo_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("p.sqlite");
        let p = p.to_str().unwrap();
        Db::apri(p).unwrap();
        let db = Db::apri(p).unwrap();
        assert_eq!(
            db.con(versione_schema).unwrap(),
            VERSIONE_SCHEMA.to_string()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
