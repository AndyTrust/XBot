//! Richieste all'utente (permessi, revisione, moduli, segreti, cassaforte, domande).
//!
//! Ogni richiesta ha una riga `richiesta` nella sua sessione (corpo `{richiesta_id, tipo,
//! carico, stato, scade_ms}`): la chat la mostra in linea e la riga si riscrive quando la
//! richiesta si risolve. Il carico non contiene mai valori segreti.

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use xbot_core::{
    Codice, Errore, GenereRiga, Ms, Risposta, Ruolo, StatoRichiesta, TipoRichiesta, nuovo_id,
    ora_ms,
};

use crate::util::{RisultatoDb, in_transazione, tutte_json, una_json};

/// Una richiesta nuova.
#[derive(Debug, Clone)]
pub struct NuovaRichiesta {
    pub sessione_id: String,
    pub tipo: TipoRichiesta,
    /// Cosa si chiede: per i permessi `{azione, destinatario?, risorsa?, comando?, descrizione?}`.
    pub carico: Value,
    pub scade_ms: Option<Ms>,
    pub piattaforma: Option<String>,
}

const SELEZIONE: &str = "SELECT r.id, r.sessione_id, s.assistente_id, r.riga_id, r.tipo,
    r.carico_json, r.stato, r.creata_ms, r.scade_ms, r.risolta_ms, r.risoluzione_json, r.piattaforma
    FROM richiesta r JOIN sessione s ON s.id = r.sessione_id";

fn corpo_riga(r: &Value) -> Value {
    json!({
        "richiesta_id": r["id"], "tipo": r["tipo"], "carico": r["carico_json"],
        "stato": r["stato"], "scade_ms": r["scade_ms"], "risoluzione": r["risoluzione_json"],
    })
}

/// Crea la richiesta e la sua riga. Restituisce (richiesta, aggiornata_seq della riga).
pub fn crea(conn: &Connection, n: &NuovaRichiesta) -> Risposta<(Value, i64)> {
    crate::sessioni::leggi(conn, &n.sessione_id)?;
    let id = nuovo_id();
    in_transazione(conn, || {
        conn.execute(
            "INSERT INTO richiesta (id, sessione_id, tipo, carico_json, creata_ms, scade_ms, piattaforma)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                n.sessione_id,
                n.tipo.come_testo(),
                n.carico.to_string(),
                ora_ms(),
                n.scade_ms,
                n.piattaforma
            ],
        )
        .db()?;
        let r = leggi(conn, &id)?;
        let riga = crate::righe::aggiungi(
            conn,
            &n.sessione_id,
            GenereRiga::Richiesta,
            Some(Ruolo::Sistema),
            &corpo_riga(&r),
        )?;
        conn.execute(
            "UPDATE richiesta SET riga_id = ?2 WHERE id = ?1",
            params![id, riga.id],
        )
        .db()?;
        Ok((leggi(conn, &id)?, riga.aggiornata_seq))
    })
}

pub fn leggi(conn: &Connection, id: &str) -> Risposta<Value> {
    una_json(conn, &format!("{SELEZIONE} WHERE r.id = ?1"), [id])?
        .ok_or_else(|| Errore::nuovo(Codice::RichiestaNonTrovata, "richiesta non trovata"))
}

/// Le richieste in uno stato (o tutte), le più recenti prima.
pub fn elenca(conn: &Connection, stato: Option<StatoRichiesta>) -> Risposta<Vec<Value>> {
    tutte_json(
        conn,
        &format!(
            "{SELEZIONE} WHERE (?1 IS NULL OR r.stato = ?1) ORDER BY r.creata_ms DESC LIMIT 500"
        ),
        [stato.map(StatoRichiesta::come_testo)],
    )
}

/// Le richieste ancora aperte di una sessione.
pub fn aperte(conn: &Connection, sessione_id: &str) -> Risposta<Vec<Value>> {
    tutte_json(
        conn,
        &format!(
            "{SELEZIONE} WHERE r.sessione_id = ?1 AND r.stato = 'aperta' ORDER BY r.creata_ms"
        ),
        [sessione_id],
    )
}

/// Chiude una richiesta aperta (`approvata`, `negata`, `scaduta`, `ritirata`) e riscrive la
/// sua riga. Una richiesta già chiusa → `richiesta_gia_risolta`. Restituisce (richiesta,
/// aggiornata_seq della riga riscritta, se c'è).
pub fn chiudi(
    conn: &Connection,
    id: &str,
    esito: StatoRichiesta,
    risoluzione: &Value,
) -> Risposta<(Value, Option<i64>)> {
    if esito == StatoRichiesta::Aperta {
        return Err(Errore::non_valida("una richiesta non si riapre"));
    }
    in_transazione(conn, || {
        let attuale = leggi(conn, id)?;
        if attuale["stato"] != "aperta" {
            // Ritirata (turno interrotto) o scaduta: non c'è più nessuno che aspetta.
            let chiusa = matches!(attuale["stato"].as_str(), Some("ritirata" | "scaduta"));
            return Err(Errore::nuovo(
                if chiusa {
                    Codice::RichiestaChiusa
                } else {
                    Codice::RichiestaGiaRisolta
                },
                format!(
                    "la richiesta è già {}",
                    attuale["stato"].as_str().unwrap_or("chiusa")
                ),
            ));
        }
        conn.execute(
            "UPDATE richiesta SET stato = ?2, risolta_ms = ?3, risoluzione_json = ?4 WHERE id = ?1",
            params![id, esito.come_testo(), ora_ms(), risoluzione.to_string()],
        )
        .db()?;
        let r = leggi(conn, id)?;
        let seq = match r["riga_id"].as_str() {
            Some(riga) => Some(crate::righe::riscrivi(conn, riga, &corpo_riga(&r))?.aggiornata_seq),
            None => None,
        };
        Ok((r, seq))
    })
}

/// Lo stato attuale di una richiesta (per chi aspetta la risposta).
pub fn stato(conn: &Connection, id: &str) -> Risposta<Option<(String, Value)>> {
    conn.query_row(
        "SELECT stato, risoluzione_json FROM richiesta WHERE id = ?1",
        [id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
    )
    .optional()
    .db()
    .map(|o| {
        o.map(|(s, j)| {
            (
                s,
                j.and_then(|j| serde_json::from_str(&j).ok())
                    .unwrap_or(Value::Null),
            )
        })
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::assistenti::{NuovoAssistente, crea as crea_assistente};
    use crate::sessioni::{self, NuovaSessione};

    #[test]
    fn crea_elenca_chiudi() {
        let db = crate::db_prova();
        db.con(|c| {
            assert!(aperte(c, "x")?.is_empty());
            let a = crea_assistente(
                c,
                &NuovoAssistente {
                    nome: "J".into(),
                    ..Default::default()
                },
            )?;
            let s = sessioni::crea(c, a["id"].as_str().unwrap(), &NuovaSessione::default())?;
            let (r, _) = crea(
                c,
                &NuovaRichiesta {
                    sessione_id: s.id.clone(),
                    tipo: TipoRichiesta::PermessoLocale,
                    carico: json!({"azione": "run-command", "comando": "ls"}),
                    scade_ms: None,
                    piattaforma: None,
                },
            )?;
            let id = r["id"].as_str().unwrap().to_string();
            assert_eq!(r["assistente_id"], a["id"]);
            assert_eq!(elenca(c, Some(StatoRichiesta::Aperta))?.len(), 1);
            let (r, seq) = chiudi(c, &id, StatoRichiesta::Approvata, &json!({"sempre": false}))?;
            assert_eq!(r["stato"], "approvata");
            assert!(seq.is_some());
            let corpo: String = c
                .query_row(
                    "SELECT corpo_json FROM riga WHERE id = ?1",
                    [r["riga_id"].as_str().unwrap()],
                    |x| x.get(0),
                )
                .unwrap();
            assert!(corpo.contains("\"approvata\""));
            let e = chiudi(c, &id, StatoRichiesta::Negata, &Value::Null).unwrap_err();
            assert_eq!(e.codice, Codice::RichiestaGiaRisolta);
            assert_eq!(stato(c, &id)?.unwrap().0, "approvata");
            Ok(())
        })
        .unwrap();
    }
}
