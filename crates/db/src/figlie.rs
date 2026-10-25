//! Sessioni figlie: un lavoro lungo in una chat a parte. Il padre ne vede solo stato e riassunto.
//! Limiti: profondità 2 (principale → figlia → nipote), al più 4 figlie vive per padre.

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use xbot_core::{Codice, Errore, Risposta, ora_ms};

use crate::sessioni::{self, NuovaSessione, Sessione};
use crate::util::{RisultatoDb, in_transazione, tutte_json};

pub const PROFONDITA_MAX: usize = 2;
pub const FIGLIE_VIVE_MAX: i64 = 4;
/// Lunghezza massima del riassunto che sale al padre.
pub const RIASSUNTO_MAX: usize = 600;

pub const STATI: &[&str] = &["al_lavoro", "in_attesa", "finita", "fallita"];

/// Quanti antenati ha una sessione (0 = principale).
pub fn profondita(conn: &Connection, id: &str) -> Risposta<usize> {
    let mut n = 0;
    let mut cur = id.to_string();
    while let Some(p) = conn
        .query_row(
            "SELECT sessione_padre_id FROM sessione WHERE id = ?1",
            [&cur],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()
        .db()?
        .flatten()
    {
        n += 1;
        cur = p;
        if n > 16 {
            break;
        }
    }
    Ok(n)
}

/// Crea una figlia di `padre_id` per `assistente_id` (di norma lo stesso del padre). Non
/// diventa la sessione corrente dell'assistente: la chat principale resta com'è.
pub fn crea(
    conn: &Connection,
    padre_id: &str,
    assistente_id: &str,
    titolo: Option<&str>,
) -> Risposta<Sessione> {
    sessioni::leggi(conn, padre_id)?;
    if profondita(conn, padre_id)? + 1 > PROFONDITA_MAX {
        return Err(Errore::nuovo(
            Codice::RichiestaNonValida,
            "troppe chat annidate: la profondità massima è 2",
        ));
    }
    let vive: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sessione WHERE sessione_padre_id = ?1
               AND figlia_stato IN ('al_lavoro','in_attesa') AND stato = 'attiva'",
            [padre_id],
            |r| r.get(0),
        )
        .db()?;
    if vive >= FIGLIE_VIVE_MAX {
        return Err(Errore::nuovo(
            Codice::RichiestaNonValida,
            "ci sono già 4 chat figlie al lavoro: aspetta che una finisca",
        ));
    }
    in_transazione(conn, || {
        let prima: Option<String> = conn
            .query_row(
                "SELECT sessione_corrente_id FROM stato_lettura WHERE assistente_id = ?1",
                [assistente_id],
                |r| r.get(0),
            )
            .optional()
            .db()?
            .flatten();
        let s = sessioni::crea(
            conn,
            assistente_id,
            &NuovaSessione {
                titolo: titolo.map(str::to_string),
                ..Default::default()
            },
        )?;
        conn.execute(
            "UPDATE sessione SET sessione_padre_id = ?2, figlia_stato = 'al_lavoro' WHERE id = ?1",
            params![s.id, padre_id],
        )
        .db()?;
        match prima {
            Some(p) => conn
                .execute(
                    "UPDATE stato_lettura SET sessione_corrente_id = ?2 WHERE assistente_id = ?1",
                    params![assistente_id, p],
                )
                .db()?,
            None => conn
                .execute(
                    "UPDATE stato_lettura SET sessione_corrente_id = NULL WHERE assistente_id = ?1",
                    [assistente_id],
                )
                .db()?,
        };
        sessioni::leggi(conn, &s.id)
    })
}

/// Cambia lo stato della figlia e il riassunto (accorciato). Non fa nulla su una sessione
/// che non è una figlia. Restituisce l'id del padre se lo stato è cambiato davvero.
pub fn imposta_stato(
    conn: &Connection,
    figlia_id: &str,
    stato: &str,
    riassunto: Option<&str>,
) -> Risposta<Option<String>> {
    if !STATI.contains(&stato) {
        return Err(Errore::nuovo(
            Codice::RichiestaNonValida,
            "stato della figlia non valido",
        ));
    }
    let r = riassunto.map(|t| t.chars().take(RIASSUNTO_MAX).collect::<String>());
    let padre: Option<(String, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT sessione_padre_id, figlia_stato, figlia_riassunto FROM sessione
              WHERE id = ?1 AND sessione_padre_id IS NOT NULL",
            [figlia_id],
            |x| Ok((x.get(0)?, x.get(1)?, x.get(2)?)),
        )
        .optional()
        .db()?;
    let Some((padre_id, vecchio, vecchio_r)) = padre else {
        return Ok(None);
    };
    if vecchio.as_deref() == Some(stato) && (r.is_none() || r == vecchio_r) {
        return Ok(None);
    }
    conn.execute(
        "UPDATE sessione SET figlia_stato = ?2, figlia_riassunto = COALESCE(?3, figlia_riassunto),
           aggiornata_ms = ?4 WHERE id = ?1",
        params![figlia_id, stato, r, ora_ms()],
    )
    .db()?;
    Ok(Some(padre_id))
}

/// Cosa è cambiato in una figlia dopo un ricalcolo.
#[derive(Debug, Clone)]
pub struct Cambio {
    pub padre_id: String,
    pub figlia_id: String,
    pub assistente_id: String,
    pub titolo: Option<String>,
    pub stato: String,
    pub riassunto: Option<String>,
    /// Vero se lo stato o il riassunto sono diversi da prima.
    pub cambiato: bool,
    /// `aggiornata_seq` della riga riscritta nel padre.
    pub seq_padre: i64,
}

/// Ricalcola lo stato di una figlia dai fatti (richieste aperte, turni) e riscrive la sua riga
/// `evento` nel padre (la crea se manca). `testo` è il risultato del turno appena chiuso.
/// `None` se la sessione non è una figlia.
pub fn ricalcola(
    conn: &Connection,
    figlia_id: &str,
    testo: Option<&str>,
) -> Risposta<Option<Cambio>> {
    type Letto = (
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let letto: Option<Letto> = conn
        .query_row(
            "SELECT sessione_padre_id, assistente_id, titolo, figlia_stato, figlia_riassunto
               FROM sessione WHERE id = ?1 AND sessione_padre_id IS NOT NULL",
            [figlia_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()
        .db()?;
    let Some((padre_id, assistente_id, titolo, vecchio, vecchio_r)) = letto else {
        return Ok(None);
    };
    let aperte = crate::richieste::aperte(conn, figlia_id)?;
    let in_corso: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM turno WHERE sessione_id = ?1 AND stato = 'in_corso'",
            [figlia_id],
            |r| r.get(0),
        )
        .db()?;
    let ultimo: Option<String> = conn
        .query_row(
            "SELECT stato FROM turno WHERE sessione_id = ?1 ORDER BY iniziato_ms DESC LIMIT 1",
            [figlia_id],
            |r| r.get(0),
        )
        .optional()
        .db()?;
    let stato = if !aperte.is_empty() {
        "in_attesa"
    } else if in_corso > 0 || ultimo.is_none() {
        "al_lavoro"
    } else if ultimo.as_deref() == Some("riuscito") {
        "finita"
    } else {
        "fallita"
    };
    let finito = matches!(stato, "finita" | "fallita");
    let riassunto: Option<String> = match testo.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) if finito => Some(t.chars().take(RIASSUNTO_MAX).collect()),
        _ => vecchio_r.clone(),
    };
    let cambiato = vecchio.as_deref() != Some(stato) || riassunto != vecchio_r;
    in_transazione(conn, || {
        if cambiato {
            conn.execute(
                "UPDATE sessione SET figlia_stato = ?2, figlia_riassunto = ?3 WHERE id = ?1",
                params![figlia_id, stato, riassunto],
            )
            .db()?;
        }
        let corpo = serde_json::json!({
            "codice": "figlia", "figlia_id": figlia_id, "assistente_id": assistente_id,
            "titolo": titolo, "stato": stato, "riassunto": riassunto,
            "richieste": aperte.iter().map(|r| serde_json::json!({
                "id": r["id"], "tipo": r["tipo"], "carico": r["carico_json"],
            })).collect::<Vec<_>>(),
        });
        let riga: Option<String> = conn
            .query_row(
                "SELECT id FROM riga WHERE sessione_id = ?1 AND genere = 'evento'
                   AND json_extract(corpo_json, '$.figlia_id') = ?2",
                params![padre_id, figlia_id],
                |r| r.get(0),
            )
            .optional()
            .db()?;
        let r = match riga {
            Some(id) => crate::righe::riscrivi(conn, &id, &corpo)?,
            None => crate::righe::aggiungi(
                conn,
                &padre_id,
                xbot_core::GenereRiga::Evento,
                Some(xbot_core::Ruolo::Sistema),
                &corpo,
            )?,
        };
        Ok(Some(Cambio {
            padre_id: padre_id.clone(),
            figlia_id: figlia_id.to_string(),
            assistente_id: assistente_id.clone(),
            titolo: titolo.clone(),
            stato: stato.to_string(),
            riassunto: riassunto.clone(),
            cambiato,
            seq_padre: r.aggiornata_seq,
        }))
    })
}

/// Le figlie di una sessione, con stato e riassunto.
pub fn elenco(conn: &Connection, padre_id: &str) -> Risposta<Vec<Value>> {
    tutte_json(
        conn,
        "SELECT id, assistente_id, titolo, figlia_stato, figlia_riassunto, creata_ms, aggiornata_ms
           FROM sessione WHERE sessione_padre_id = ?1 ORDER BY creata_ms",
        [padre_id],
    )
}

/// Il padre e, se serve, il nonno: dove devono comparire le richieste di una figlia.
pub fn antenati(conn: &Connection, id: &str) -> Risposta<Vec<String>> {
    let mut v = Vec::new();
    let mut cur = id.to_string();
    while let Some(p) = conn
        .query_row(
            "SELECT sessione_padre_id FROM sessione WHERE id = ?1",
            [&cur],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()
        .db()?
        .flatten()
    {
        v.push(p.clone());
        cur = p;
        if v.len() > 16 {
            break;
        }
    }
    Ok(v)
}

#[cfg(test)]
mod test {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::prepara(&c).unwrap();
        c.execute(
            "INSERT INTO assistente (id, nome, creato_ms, aggiornato_ms) VALUES ('a1','A',0,0)",
            [],
        )
        .unwrap();
        c
    }

    #[test]
    fn figlia_non_prende_la_chat_corrente() {
        let c = db();
        let p = sessioni::crea(&c, "a1", &NuovaSessione::default()).unwrap();
        let f = crea(&c, &p.id, "a1", Some("lavoro")).unwrap();
        assert_eq!(f.sessione_padre_id.as_deref(), Some(p.id.as_str()));
        let corr: String = c
            .query_row(
                "SELECT sessione_corrente_id FROM stato_lettura WHERE assistente_id='a1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(corr, p.id);
        let e = elenco(&c, &p.id).unwrap();
        assert_eq!(e[0]["figlia_stato"], "al_lavoro");
    }

    #[test]
    fn profondita_e_numero_massimi() {
        let c = db();
        let p = sessioni::crea(&c, "a1", &NuovaSessione::default()).unwrap();
        let f = crea(&c, &p.id, "a1", None).unwrap();
        let n = crea(&c, &f.id, "a1", None).unwrap();
        assert_eq!(profondita(&c, &n.id).unwrap(), 2);
        assert_eq!(antenati(&c, &n.id).unwrap(), [f.id.clone(), p.id.clone()]);
        assert!(crea(&c, &n.id, "a1", None).is_err());
        for _ in 0..3 {
            crea(&c, &p.id, "a1", None).unwrap();
        }
        assert!(crea(&c, &p.id, "a1", None).is_err());
        imposta_stato(&c, &f.id, "finita", Some("fatto")).unwrap();
        assert!(crea(&c, &p.id, "a1", None).is_ok());
    }

    #[test]
    fn ricalcolo_dai_fatti_e_riga_nel_padre() {
        let c = db();
        let p = sessioni::crea(&c, "a1", &NuovaSessione::default()).unwrap();
        let f = crea(&c, &p.id, "a1", Some("ricerca")).unwrap();
        let k = ricalcola(&c, &f.id, None).unwrap().unwrap();
        assert_eq!(k.stato, "al_lavoro");
        assert_eq!(ricalcola(&c, &p.id, None).unwrap().map(|x| x.stato), None);
        // una richiesta aperta: in attesa; la riga del padre è la stessa, riscritta
        let (r, _) = crate::richieste::crea(
            &c,
            &crate::richieste::NuovaRichiesta {
                sessione_id: f.id.clone(),
                tipo: xbot_core::TipoRichiesta::PermessoLocale,
                carico: serde_json::json!({"azione": "write-file"}),
                scade_ms: None,
                piattaforma: None,
            },
        )
        .unwrap();
        let k = ricalcola(&c, &f.id, None).unwrap().unwrap();
        assert_eq!(k.stato, "in_attesa");
        assert!(k.cambiato);
        let n: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM riga WHERE sessione_id = ?1 AND genere = 'evento'",
                [&p.id],
                |x| x.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
        let corpo: String = c
            .query_row(
                "SELECT corpo_json FROM riga WHERE sessione_id = ?1 AND genere = 'evento'",
                [&p.id],
                |x| x.get(0),
            )
            .unwrap();
        assert!(corpo.contains(r["id"].as_str().unwrap()), "{corpo}");
        // senza novità non cambia
        assert!(!ricalcola(&c, &f.id, None).unwrap().unwrap().cambiato);
    }

    #[test]
    fn stato_cambia_una_volta() {
        let c = db();
        let p = sessioni::crea(&c, "a1", &NuovaSessione::default()).unwrap();
        let f = crea(&c, &p.id, "a1", None).unwrap();
        assert_eq!(
            imposta_stato(&c, &f.id, "in_attesa", None)
                .unwrap()
                .as_deref(),
            Some(p.id.as_str())
        );
        assert_eq!(imposta_stato(&c, &f.id, "in_attesa", None).unwrap(), None);
        assert_eq!(imposta_stato(&c, &p.id, "finita", None).unwrap(), None);
        assert!(imposta_stato(&c, &f.id, "boh", None).is_err());
        let lungo = "x".repeat(2000);
        imposta_stato(&c, &f.id, "finita", Some(&lungo)).unwrap();
        let e = elenco(&c, &p.id).unwrap();
        assert_eq!(
            e[0]["figlia_riassunto"].as_str().unwrap().len(),
            RIASSUNTO_MAX
        );
    }
}
