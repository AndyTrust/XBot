//! Lavori (Fase 2.E S-B): un turno spedito a un computer collegato. Il carico salvato è senza
//! segreti (quelli viaggiano solo nel frame `lavoro`, in memoria); gli eventi del lavoratore si
//! salvano in `lavoro_evento` con il loro `seq`, così la riconnessione non li applica due volte.

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use xbot_core::{Codice, Errore, Risposta, StatoLavoro, nuovo_id, ora_ms};

use crate::util::{RisultatoDb, tutte_json, una_json};

/// Un lavoro nuovo.
#[derive(Debug, Clone)]
pub struct NuovoLavoro<'a> {
    pub macchina_id: &'a str,
    pub sessione_id: &'a str,
    pub turno_id: &'a str,
    /// Il carico senza segreti.
    pub carico: &'a Value,
}

/// Il lavoro scritto: id, generazione (cresce per sessione) e recinto (= la sessione).
#[derive(Debug, Clone, PartialEq)]
pub struct LavoroCreato {
    pub id: String,
    pub generazione: i64,
    pub recinto_id: String,
}

pub fn crea(conn: &Connection, n: &NuovoLavoro<'_>) -> Risposta<LavoroCreato> {
    let generazione: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(generazione), 0) + 1 FROM lavoro WHERE sessione_id = ?1",
            [n.sessione_id],
            |r| r.get(0),
        )
        .db()?;
    let id = nuovo_id();
    conn.execute(
        "INSERT INTO lavoro (id, macchina_id, sessione_id, turno_id, capacita, carico_json,
           generazione, recinto_id, stato, creato_ms)
         VALUES (?1, ?2, ?3, ?4, 'turni', ?5, ?6, ?3, 'in_coda', ?7)",
        params![
            id,
            n.macchina_id,
            n.sessione_id,
            n.turno_id,
            n.carico.to_string(),
            generazione,
            ora_ms()
        ],
    )
    .db()?;
    Ok(LavoroCreato {
        id,
        generazione,
        recinto_id: n.sessione_id.to_string(),
    })
}

pub fn leggi(conn: &Connection, id: &str) -> Risposta<Value> {
    una_json(
        conn,
        "SELECT id, macchina_id, sessione_id, turno_id, capacita, generazione, recinto_id,
           tentativo, lease_scade_ms, stato, creato_ms, preso_ms, finito_ms, codice_uscita, esito_json
         FROM lavoro WHERE id = ?1",
        [id],
    )?
    .ok_or_else(|| Errore::nuovo(Codice::NonTrovato, "lavoro non trovato"))
}

/// Cambia lo stato. `preso`/`in_corso` scrivono `preso_ms`, gli stati finali `finito_ms`.
/// Un lavoro già finito non cambia più. Restituisce vero se ha cambiato.
pub fn segna(
    conn: &Connection,
    id: &str,
    stato: StatoLavoro,
    esito: Option<&Value>,
) -> Risposta<bool> {
    let ora = ora_ms();
    let finale = matches!(
        stato,
        StatoLavoro::Riuscito
            | StatoLavoro::Fallito
            | StatoLavoro::Annullato
            | StatoLavoro::Scaduto
    );
    let n = conn
        .execute(
            "UPDATE lavoro SET stato = ?2,
               preso_ms = CASE WHEN ?2 IN ('preso','in_corso') THEN COALESCE(preso_ms, ?3) ELSE preso_ms END,
               finito_ms = CASE WHEN ?4 THEN ?3 ELSE finito_ms END,
               esito_json = COALESCE(?5, esito_json)
             WHERE id = ?1 AND stato IN ('in_coda','preso','in_corso')",
            params![
                id,
                stato.come_testo(),
                ora,
                finale,
                esito.map(Value::to_string)
            ],
        )
        .db()?;
    Ok(n > 0)
}

/// Il lease dei lavori presi da una macchina: scade a `scade_ms` (ultimo battito + 60 s).
pub fn rinnova_lease(conn: &Connection, macchina_id: &str, scade_ms: i64) -> Risposta<()> {
    conn.execute(
        "UPDATE lavoro SET lease_scade_ms = ?2
         WHERE macchina_id = ?1 AND stato IN ('preso','in_corso')",
        params![macchina_id, scade_ms],
    )
    .db()?;
    Ok(())
}

/// Salva un evento del lavoratore. Falso se quel `seq` c'era già (rimandato dal giornale).
pub fn registra_evento(
    conn: &Connection,
    lavoro_id: &str,
    seq: i64,
    tipo: &str,
    dati: &Value,
) -> Risposta<bool> {
    let n = conn
        .execute(
            "INSERT INTO lavoro_evento (lavoro_id, seq, tipo, dati, ms) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT DO NOTHING",
            params![lavoro_id, seq, tipo, dati.to_string(), ora_ms()],
        )
        .db()?;
    Ok(n > 0)
}

/// L'ultimo `seq` confermato di un lavoro (0 se nessuno).
pub fn fino_a_seq(conn: &Connection, lavoro_id: &str) -> Risposta<i64> {
    conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) FROM lavoro_evento WHERE lavoro_id = ?1",
        [lavoro_id],
        |r| r.get(0),
    )
    .db()
}

/// I lavori non finiti di una macchina, dal più vecchio.
pub fn aperti(conn: &Connection, macchina_id: &str) -> Risposta<Vec<Value>> {
    tutte_json(
        conn,
        "SELECT id, sessione_id, turno_id, generazione, stato, lease_scade_ms, creato_ms,
           (SELECT COALESCE(MAX(seq), 0) FROM lavoro_evento e WHERE e.lavoro_id = l.id) AS fino_a_seq
         FROM lavoro l WHERE macchina_id = ?1 AND stato IN ('in_coda','preso','in_corso')
         ORDER BY creato_ms",
        [macchina_id],
    )
}

/// Il lavoro di un turno, se c'è.
pub fn per_turno(conn: &Connection, turno_id: &str) -> Risposta<Option<String>> {
    conn.query_row(
        "SELECT id FROM lavoro WHERE turno_id = ?1 ORDER BY creato_ms DESC, generazione DESC LIMIT 1",
        [turno_id],
        |r| r.get(0),
    )
    .optional()
    .db()
}

/// All'avvio del server: i lavori rimasti aperti non hanno più un turno vivo (i turni orfani
/// sono già chiusi come interrotti) e diventano `annullato`.
pub fn chiudi_orfani(conn: &Connection) -> Risposta<usize> {
    conn.execute(
        "UPDATE lavoro SET stato = 'annullato', finito_ms = ?1
         WHERE stato IN ('in_coda','preso','in_corso')",
        [ora_ms()],
    )
    .db()
}

/// Mette in coda un frame `risolta` che non è partito (computer scollegato). Un secondo
/// frame con lo stesso `richiesta_id` non sostituisce il primo.
pub fn accoda_risolta(
    conn: &Connection,
    macchina_id: &str,
    lavoro_id: Option<&str>,
    richiesta_id: &str,
    frame: &Value,
) -> Risposta<()> {
    conn.execute(
        "INSERT OR IGNORE INTO risolta_in_coda (richiesta_id, macchina_id, lavoro_id, frame_json,
           creata_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            richiesta_id,
            macchina_id,
            lavoro_id,
            frame.to_string(),
            ora_ms()
        ],
    )
    .db()?;
    Ok(())
}

/// Prende i frame `risolta` in coda di una macchina e li segna consegnati nello stesso
/// UPDATE (chi li prende li manda: partono una volta sola). In ordine di creazione.
pub fn prendi_risolte(conn: &Connection, macchina_id: &str) -> Risposta<Vec<Value>> {
    let mut st = conn
        .prepare(
            "UPDATE risolta_in_coda SET consegnata_ms = ?2
             WHERE macchina_id = ?1 AND consegnata_ms IS NULL
             RETURNING creata_ms, rowid, frame_json",
        )
        .db()?;
    let mut v: Vec<(i64, i64, String)> = st
        .query_map(params![macchina_id, ora_ms()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .db()?
        .collect::<Result<_, _>>()
        .db()?;
    v.sort();
    Ok(v.into_iter()
        .filter_map(|(_, _, f)| serde_json::from_str(&f).ok())
        .collect())
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json::json;

    #[test]
    fn crea_segna_ed_eventi() {
        let db = crate::db_prova();
        db.con(|c| {
            crate::macchine::registra_se_stessa(c, "m1", "mac", "mac", "macos", "aarch64")?;
            let a = crate::assistenti::crea(
                c,
                &crate::assistenti::NuovoAssistente {
                    nome: "Prova".into(),
                    ..Default::default()
                },
            )?;
            let s = crate::sessioni::crea(c, a["id"].as_str().unwrap_or(""), &Default::default())?;
            crate::provider::registra(c, "finto", "Finto", "interno")?;
            let t = crate::turni::crea(
                c,
                &crate::turni::NuovoTurno {
                    sessione_id: s.id.clone(),
                    invio_id: None,
                    provider_id: "finto".into(),
                    modello_id: None,
                    livello: None,
                    macchina_id: Some("m1".into()),
                },
            )?;
            let carico = json!({"testo": "ciao"});
            let n = NuovoLavoro {
                macchina_id: "m1",
                sessione_id: &s.id,
                turno_id: &t.id,
                carico: &carico,
            };
            let l1 = crea(c, &n)?;
            let l2 = crea(c, &n)?;
            assert_eq!((l1.generazione, l2.generazione), (1, 2));
            assert_eq!(l1.recinto_id, s.id);
            assert_eq!(aperti(c, "m1")?.len(), 2);
            assert!(segna(c, &l1.id, StatoLavoro::Preso, None)?);
            rinnova_lease(c, "m1", 99)?;
            assert_eq!(leggi(c, &l1.id)?["lease_scade_ms"], 99);
            assert!(registra_evento(
                c,
                &l1.id,
                1,
                "evento",
                &json!({"tipo": "avviato"})
            )?);
            assert!(!registra_evento(c, &l1.id, 1, "evento", &json!({}))?);
            assert!(registra_evento(c, &l1.id, 2, "evento", &json!({}))?);
            assert_eq!(fino_a_seq(c, &l1.id)?, 2);
            assert!(segna(
                c,
                &l1.id,
                StatoLavoro::Riuscito,
                Some(&json!({"ok": true}))
            )?);
            assert!(!segna(c, &l1.id, StatoLavoro::Fallito, None)?);
            assert_eq!(leggi(c, &l1.id)?["stato"], "riuscito");
            assert_eq!(per_turno(c, &t.id)?.as_deref(), Some(l2.id.as_str()));
            assert_eq!(chiudi_orfani(c)?, 1);
            assert!(aperti(c, "m1")?.is_empty());
            // Le risolte in coda partono una volta sola, in ordine.
            accoda_risolta(c, "m1", Some(&l1.id), "r1", &json!({"richiesta_id": "r1"}))?;
            accoda_risolta(c, "m1", None, "r2", &json!({"richiesta_id": "r2"}))?;
            accoda_risolta(c, "m1", None, "r1", &json!({"richiesta_id": "doppio"}))?;
            let v = prendi_risolte(c, "m1")?;
            assert_eq!(
                v,
                vec![json!({"richiesta_id": "r1"}), json!({"richiesta_id": "r2"})]
            );
            assert!(prendi_risolte(c, "m1")?.is_empty());
            Ok(())
        })
        .unwrap();
    }
}
