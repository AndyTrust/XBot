//! Sessioni di un assistente: creazione, lettura, modifica, azzeramento (nuova generazione).

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use xbot_core::{Codice, Errore, Id, Ms, Risposta, nuovo_id, ora_ms};

use crate::controlli;
use crate::util::{RisultatoDb, aggiorna_campi, in_transazione};

#[derive(Debug, Clone, Serialize)]
pub struct Sessione {
    pub id: Id,
    pub assistente_id: Id,
    pub progetto_id: Option<String>,
    pub titolo: Option<String>,
    pub stato: String,
    pub provider_id: Option<String>,
    pub modello_id: Option<String>,
    pub livello_ragionamento: Option<String>,
    pub macchina_id: Option<String>,
    pub cartella_lavoro: Option<String>,
    pub sessione_esterna_id: Option<String>,
    pub generazione: i64,
    pub ultimo_seq: i64,
    pub ultimo_aggiornata_seq: i64,
    pub sessione_padre_id: Option<String>,
    pub creata_ms: Ms,
    pub aggiornata_ms: Ms,
    pub archiviata_ms: Option<Ms>,
}

const COLONNE: &str = "id, assistente_id, progetto_id, titolo, stato, provider_id, modello_id, \
    livello_ragionamento, macchina_id, cartella_lavoro, sessione_esterna_id, generazione, \
    ultimo_seq, ultimo_aggiornata_seq, sessione_padre_id, creata_ms, aggiornata_ms, archiviata_ms";

fn da_riga(r: &Row<'_>) -> rusqlite::Result<Sessione> {
    Ok(Sessione {
        id: r.get(0)?,
        assistente_id: r.get(1)?,
        progetto_id: r.get(2)?,
        titolo: r.get(3)?,
        stato: r.get(4)?,
        provider_id: r.get(5)?,
        modello_id: r.get(6)?,
        livello_ragionamento: r.get(7)?,
        macchina_id: r.get(8)?,
        cartella_lavoro: r.get(9)?,
        sessione_esterna_id: r.get(10)?,
        generazione: r.get(11)?,
        ultimo_seq: r.get(12)?,
        ultimo_aggiornata_seq: r.get(13)?,
        sessione_padre_id: r.get(14)?,
        creata_ms: r.get(15)?,
        aggiornata_ms: r.get(16)?,
        archiviata_ms: r.get(17)?,
    })
}

/// I dati di una sessione nuova. I campi vuoti prendono i predefiniti dell'assistente.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct NuovaSessione {
    pub titolo: Option<String>,
    pub provider_id: Option<String>,
    pub modello_id: Option<String>,
    pub livello_ragionamento: Option<String>,
    pub macchina_id: Option<String>,
    pub cartella_lavoro: Option<String>,
    pub progetto_id: Option<String>,
}

pub fn crea(conn: &Connection, assistente_id: &str, n: &NuovaSessione) -> Risposta<Sessione> {
    for (campo, valore) in [
        ("provider_id", &n.provider_id),
        ("modello_id", &n.modello_id),
        ("macchina_id", &n.macchina_id),
        ("progetto_id", &n.progetto_id),
    ] {
        controlli::riferimento(conn, campo, valore.as_deref())?;
    }
    controlli::testo_valido("titolo", n.titolo.as_deref(), true)?;
    controlli::lunghezza("titolo", n.titolo.as_deref(), controlli::TITOLO_SESSIONE)?;
    controlli::assistente_attivo(conn, assistente_id)?;
    // --- S-B (2.E) --- anche la macchina predefinita dell'assistente.
    type Predefiniti = (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let predefiniti: Option<Predefiniti> = conn
        .query_row(
            "SELECT provider_predefinito_id, modello_predefinito_id, livello_ragionamento,
               macchina_predefinita_id
               FROM assistente WHERE id = ?1 AND archiviato_ms IS NULL",
            [assistente_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .db()?;
    let Some((prov, mod_, liv, macch)) = predefiniti else {
        return Err(Errore::nuovo(
            Codice::AssistenteNonTrovato,
            "assistente non trovato",
        ));
    };
    let id = nuovo_id();
    let ora = ora_ms();
    in_transazione(conn, || {
        conn.execute(
            "INSERT INTO sessione (id, assistente_id, progetto_id, titolo, provider_id, modello_id,
               livello_ragionamento, macchina_id, cartella_lavoro, creata_ms, aggiornata_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
            params![
                id,
                assistente_id,
                n.progetto_id,
                n.titolo,
                n.provider_id.clone().or(prov),
                n.modello_id.clone().or(mod_),
                n.livello_ragionamento.clone().or(liv),
                n.macchina_id.clone().or(macch),
                n.cartella_lavoro,
                ora
            ],
        )
        .db()?;
        conn.execute(
            "INSERT INTO stato_lettura (assistente_id, sessione_corrente_id) VALUES (?1, ?2)
             ON CONFLICT(assistente_id) DO UPDATE SET sessione_corrente_id = excluded.sessione_corrente_id",
            params![assistente_id, id],
        )
        .db()?;
        leggi(conn, &id)
    })
}

pub fn leggi(conn: &Connection, id: &str) -> Risposta<Sessione> {
    conn.query_row(
        &format!("SELECT {COLONNE} FROM sessione WHERE id = ?1"),
        [id],
        da_riga,
    )
    .optional()
    .db()?
    .ok_or_else(|| Errore::nuovo(Codice::SessioneNonTrovata, "sessione non trovata"))
}

/// Le sessioni di un assistente, le più recenti prima. `tutte` include le archiviate.
pub fn elenca(conn: &Connection, assistente_id: &str, tutte: bool) -> Risposta<Vec<Sessione>> {
    let filtro = if tutte { "" } else { "AND stato = 'attiva'" };
    let mut st = conn
        .prepare(&format!(
            "SELECT {COLONNE} FROM sessione WHERE assistente_id = ?1 {filtro}
             ORDER BY aggiornata_ms DESC"
        ))
        .db()?;
    let v = st
        .query_map([assistente_id], da_riga)
        .db()?
        .collect::<rusqlite::Result<Vec<_>>>()
        .db()?;
    Ok(v)
}

const MODIFICABILI: &[&str] = &[
    "titolo",
    "provider_id",
    "modello_id",
    "livello_ragionamento",
    "macchina_id",
    "cartella_lavoro",
    "progetto_id",
];

/// Modifica parziale. `archivia: true|false` archivia o riattiva.
pub fn aggiorna(conn: &Connection, id: &str, campi: &Map<String, Value>) -> Risposta<Sessione> {
    leggi(conn, id)?;
    controlli::riferimenti(conn, campi)?;
    controlli::nomi_validi(campi, &["titolo"])?;
    controlli::lunghezze(campi, &[("titolo", controlli::TITOLO_SESSIONE)])?;
    let mut campi = campi.clone();
    let archivia = campi.remove("archivia");
    let ora = ora_ms();
    in_transazione(conn, || {
        aggiorna_campi(
            conn,
            "sessione",
            "id",
            id,
            &campi,
            MODIFICABILI,
            &[("aggiornata_ms", Value::from(ora))],
        )?;
        match archivia {
            Some(Value::Bool(true)) => {
                conn.execute(
                    "UPDATE sessione SET stato = 'archiviata', archiviata_ms = ?2 WHERE id = ?1",
                    params![id, ora],
                )
                .db()?;
            }
            Some(Value::Bool(false)) => {
                conn.execute(
                    "UPDATE sessione SET stato = 'attiva', archiviata_ms = NULL WHERE id = ?1",
                    [id],
                )
                .db()?;
            }
            Some(_) => return Err(Errore::non_valida("archivia vuole true o false")),
            None => {}
        }
        leggi(conn, id)
    })
}

/// Azzera la cronologia: nuova generazione, righe vecchie soppresse (non cancellate),
/// sessione esterna dimenticata. Restituisce la nuova generazione.
pub fn azzera(conn: &Connection, id: &str) -> Risposta<i64> {
    in_transazione(conn, || {
        let generazione: Option<i64> = conn
            .query_row(
                "UPDATE sessione SET generazione = generazione + 1, sessione_esterna_id = NULL,
                   aggiornata_ms = ?2 WHERE id = ?1 RETURNING generazione",
                params![id, ora_ms()],
                |r| r.get(0),
            )
            .optional()
            .db()?;
        let generazione = generazione
            .ok_or_else(|| Errore::nuovo(Codice::SessioneNonTrovata, "sessione non trovata"))?;
        conn.execute("UPDATE riga SET soppressa = 1 WHERE sessione_id = ?1", [id])
            .db()?;
        Ok(generazione)
    })
}

/// Ricorda l'id della sessione nel client ufficiale (per riprenderla).
pub fn imposta_esterna(conn: &Connection, id: &str, esterna: &str) -> Risposta<()> {
    conn.execute(
        "UPDATE sessione SET sessione_esterna_id = ?2 WHERE id = ?1",
        params![id, esterna],
    )
    .db()?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use crate::assistenti::{self, NuovoAssistente};

    pub(crate) fn sessione_prova(c: &Connection) -> Sessione {
        let a = assistenti::crea(
            c,
            &NuovoAssistente {
                nome: "Atena".into(),
                ..Default::default()
            },
        )
        .unwrap();
        crea(c, a["id"].as_str().unwrap(), &NuovaSessione::default()).unwrap()
    }

    #[test]
    fn crea_modifica_archivia_azzera() {
        let db = crate::db_prova();
        db.con(|c| {
            let s = sessione_prova(c);
            assert_eq!(s.generazione, 1);
            let mut m = Map::new();
            m.insert("titolo".into(), Value::from("Prova"));
            m.insert("archivia".into(), Value::Bool(true));
            let s2 = aggiorna(c, &s.id, &m)?;
            assert_eq!(s2.titolo.as_deref(), Some("Prova"));
            assert_eq!(s2.stato, "archiviata");
            assert!(elenca(c, &s.assistente_id, false)?.is_empty());
            assert_eq!(elenca(c, &s.assistente_id, true)?.len(), 1);
            assert_eq!(azzera(c, &s.id)?, 2);
            assert!(leggi(c, "nessuna").is_err());
            Ok(())
        })
        .unwrap();
    }
}
