//! Routine (automazioni): il compito, l'attivatore, la prossima scadenza, le esecuzioni.
//! Le scadenze le calcola l'orologio del server (`orologio.rs`) e arrivano già pronte.

use rusqlite::{Connection, params};
use serde_json::{Map, Value};
use xbot_core::{Codice, Errore, Ms, Risposta, nuovo_id, ora_ms};

use crate::util::{RisultatoDb, aggiorna_campi, in_transazione, tutte_json, una_json};

pub const MAX_CORPO: usize = 20_000;

/// Una routine già controllata dal server.
#[derive(Debug, Clone)]
pub struct NuovaRoutine {
    pub nome: String,
    pub descrizione: Option<String>,
    pub corpo: String,
    pub attivatore_tipo: String,
    pub attivatore: Value,
    pub abilitata: bool,
    pub prossima_ms: Option<Ms>,
}

fn controlla_testi(
    nome: Option<&str>,
    corpo: Option<&str>,
    descrizione: Option<&str>,
) -> Risposta<()> {
    if nome.is_some_and(|n| n.trim().is_empty()) {
        return Err(Errore::non_valida("il nome è obbligatorio"));
    }
    if corpo.is_some_and(|c| c.trim().is_empty()) {
        return Err(Errore::non_valida("il compito (corpo) è obbligatorio"));
    }
    crate::controlli::testo_valido("nome", nome, true)?;
    crate::controlli::lunghezza("nome", nome, crate::controlli::NOME_ASSISTENTE)?;
    crate::controlli::lunghezza("descrizione", descrizione, crate::controlli::DESCRIZIONE)?;
    crate::controlli::lunghezza("corpo", corpo, MAX_CORPO)
}

pub fn crea(conn: &Connection, assistente_id: &str, n: &NuovaRoutine) -> Risposta<Value> {
    crate::controlli::assistente_attivo(conn, assistente_id)?;
    controlla_testi(Some(&n.nome), Some(&n.corpo), n.descrizione.as_deref())?;
    let id = nuovo_id();
    conn.execute(
        "INSERT INTO routine (id, assistente_id, nome, descrizione, corpo, attivatore_tipo,
           attivatore_json, abilitata, prossima_ms, creata_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            id,
            assistente_id,
            n.nome.trim(),
            n.descrizione,
            n.corpo,
            n.attivatore_tipo,
            n.attivatore.to_string(),
            i64::from(n.abilitata),
            n.prossima_ms,
            ora_ms()
        ],
    )
    .db()?;
    leggi(conn, &id)
}

pub fn leggi(conn: &Connection, id: &str) -> Risposta<Value> {
    una_json(conn, "SELECT * FROM routine WHERE id = ?1", [id])?
        .ok_or_else(|| Errore::nuovo(Codice::RoutineNonTrovata, "routine non trovata"))
}

pub fn elenca(conn: &Connection, assistente_id: Option<&str>) -> Risposta<Vec<Value>> {
    tutte_json(
        conn,
        "SELECT * FROM routine WHERE (?1 IS NULL OR assistente_id = ?1) ORDER BY nome",
        [assistente_id],
    )
}

const MODIFICABILI: &[&str] = &[
    "nome",
    "descrizione",
    "corpo",
    "attivatore_tipo",
    "attivatore_json",
    "abilitata",
    "prossima_ms",
];

/// Modifica parziale con colonne già controllate dal server (`attivatore_json` come testo).
pub fn aggiorna(conn: &Connection, id: &str, campi: &Map<String, Value>) -> Risposta<Value> {
    leggi(conn, id)?;
    controlla_testi(
        campi.get("nome").and_then(Value::as_str),
        campi.get("corpo").and_then(Value::as_str),
        campi.get("descrizione").and_then(Value::as_str),
    )?;
    aggiorna_campi(conn, "routine", "id", id, campi, MODIFICABILI, &[])?;
    leggi(conn, id)
}

pub fn elimina(conn: &Connection, id: &str) -> Risposta<()> {
    let n = conn
        .execute("DELETE FROM routine WHERE id = ?1", [id])
        .db()?;
    if n == 0 {
        return Err(Errore::nuovo(
            Codice::RoutineNonTrovata,
            "routine non trovata",
        ));
    }
    Ok(())
}

/// Le routine abilitate con la scadenza passata.
pub fn scadute(conn: &Connection, ora: Ms) -> Risposta<Vec<Value>> {
    tutte_json(
        conn,
        "SELECT * FROM routine WHERE abilitata = 1 AND prossima_ms IS NOT NULL AND prossima_ms <= ?1
         ORDER BY prossima_ms LIMIT 50",
        [ora],
    )
}

/// Registra un'esecuzione e sposta la scadenza. Restituisce l'id dell'esecuzione.
pub fn registra_esecuzione(
    conn: &Connection,
    routine_id: &str,
    sessione_id: Option<&str>,
    esito: &str,
    prossima_ms: Option<Ms>,
) -> Risposta<String> {
    let id = nuovo_id();
    let ora = ora_ms();
    in_transazione(conn, || {
        conn.execute(
            "INSERT INTO routine_esecuzione (id, routine_id, sessione_id, iniziata_ms, finita_ms, esito)
             VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
            params![id, routine_id, sessione_id, ora, esito],
        )
        .db()?;
        conn.execute(
            "UPDATE routine SET ultima_ms = ?2, ultimo_esito = ?3, prossima_ms = ?4 WHERE id = ?1",
            params![routine_id, ora, esito, prossima_ms],
        )
        .db()?;
        Ok(id.clone())
    })
}

/// Le ultime esecuzioni di una routine.
pub fn esecuzioni(conn: &Connection, routine_id: &str, limite: i64) -> Risposta<Vec<Value>> {
    tutte_json(
        conn,
        "SELECT * FROM routine_esecuzione WHERE routine_id = ?1 ORDER BY iniziata_ms DESC LIMIT ?2",
        params![routine_id, limite.clamp(1, 200)],
    )
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::assistenti::{NuovoAssistente, crea as crea_assistente};

    #[test]
    fn crea_scade_esegue() {
        let db = crate::db_prova();
        db.con(|c| {
            assert!(elenca(c, None)?.is_empty());
            let a = crea_assistente(
                c,
                &NuovoAssistente {
                    nome: "J".into(),
                    ..Default::default()
                },
            )?;
            let aid = a["id"].as_str().unwrap();
            let r = crea(
                c,
                aid,
                &NuovaRoutine {
                    nome: "Giro".into(),
                    descrizione: None,
                    corpo: "fai il giro".into(),
                    attivatore_tipo: "ogni_secondi".into(),
                    attivatore: serde_json::json!({"every_seconds": 5}),
                    abilitata: true,
                    prossima_ms: Some(1),
                },
            )?;
            let id = r["id"].as_str().unwrap().to_string();
            assert_eq!(r["attivatore_json"]["every_seconds"], 5);
            assert_eq!(scadute(c, 10)?.len(), 1);
            registra_esecuzione(c, &id, None, "accettato", Some(i64::MAX))?;
            assert!(scadute(c, 10)?.is_empty());
            assert_eq!(esecuzioni(c, &id, 10)?.len(), 1);
            let mut m = Map::new();
            m.insert("nome".into(), Value::from(""));
            assert!(aggiorna(c, &id, &m).is_err());
            elimina(c, &id)?;
            assert_eq!(leggi(c, &id).unwrap_err().codice, Codice::RoutineNonTrovata);
            Ok(())
        })
        .unwrap();
    }
}
