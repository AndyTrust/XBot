//! Il computer di un assistente (Fase 2.H, migrazione 009): la volontà (`assistente.computer_acceso`)
//! e lo stato dei processi (`computer_assistente`), con display e porte del suo posto.

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use xbot_core::{Risposta, ora_ms};

use crate::util::{RisultatoDb, tutte_json, una_json};

/// I posti possibili: display 50..=89, porte 5950+n, 6950+n, 7950+n, 9350+n.
pub const POSTI: i64 = 40;

/// Display e porte di un posto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Posto {
    pub display: i64,
    pub porta_vnc: i64,
    pub porta_ws: i64,
    pub porta_ttyd: i64,
    pub porta_chrome: i64,
}

impl Posto {
    pub fn numero(n: i64) -> Self {
        Self {
            display: 50 + n,
            porta_vnc: 5950 + n,
            porta_ws: 6950 + n,
            porta_ttyd: 7950 + n,
            porta_chrome: 9350 + n,
        }
    }
}

/// `acceso`, `avvio`, `spento` o `errore` (`spento` se la riga manca).
pub fn stato(conn: &Connection, assistente_id: &str) -> Risposta<String> {
    Ok(conn
        .query_row(
            "SELECT stato FROM computer_assistente WHERE assistente_id = ?1",
            [assistente_id],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .db()?
        .unwrap_or_else(|| "spento".into()))
}

/// La riga del computer, o una riga `spento` vuota.
pub fn leggi(conn: &Connection, assistente_id: &str) -> Risposta<Value> {
    crate::assistenti::leggi(conn, assistente_id)?;
    Ok(una_json(
        conn,
        "SELECT * FROM computer_assistente WHERE assistente_id = ?1",
        [assistente_id],
    )?
    .unwrap_or_else(|| {
        json!({
            "assistente_id": assistente_id, "stato": "spento", "display": null,
            "porta_vnc": null, "porta_ws": null, "porta_ttyd": null, "porta_chrome": null,
            "larghezza": 1280, "altezza": 800, "acceso_ms": null, "errore": null,
        })
    }))
}

/// La volontà dell'utente: il computer dell'assistente va tenuto acceso.
pub fn imposta_acceso(conn: &Connection, assistente_id: &str, acceso: bool) -> Risposta<()> {
    crate::assistenti::leggi(conn, assistente_id)?;
    conn.execute(
        "UPDATE assistente SET computer_acceso = ?2 WHERE id = ?1",
        params![assistente_id, i64::from(acceso)],
    )
    .db()?;
    Ok(())
}

/// Gli assistenti (non archiviati) con il computer da tenere acceso.
pub fn da_accendere(conn: &Connection) -> Risposta<Vec<String>> {
    Ok(tutte_json(
        conn,
        "SELECT id FROM assistente WHERE computer_acceso = 1 AND archiviato_ms IS NULL ORDER BY id",
        [],
    )?
    .into_iter()
    .filter_map(|v| v["id"].as_str().map(str::to_string))
    .collect())
}

/// Il posto dell'assistente: quello che ha già, o il primo libero che `libero` accetta.
pub fn assegna_posto(
    conn: &Connection,
    assistente_id: &str,
    libero: impl Fn(&Posto) -> bool,
) -> Risposta<Option<Posto>> {
    let gia: Option<i64> = conn
        .query_row(
            "SELECT display FROM computer_assistente WHERE assistente_id = ?1 AND display IS NOT NULL",
            [assistente_id],
            |r| r.get(0),
        )
        .optional()
        .db()?;
    if let Some(d) = gia {
        let p = Posto::numero(d - 50);
        if libero(&p) {
            return Ok(Some(p));
        }
    }
    let presi: Vec<i64> = tutte_json(
        conn,
        "SELECT display FROM computer_assistente WHERE display IS NOT NULL AND assistente_id != ?1",
        [assistente_id],
    )?
    .iter()
    .filter_map(|v| v["display"].as_i64())
    .collect();
    Ok((0..POSTI)
        .map(Posto::numero)
        .find(|p| !presi.contains(&p.display) && libero(p)))
}

/// Scrive lo stato (e il posto, se dato).
pub fn scrivi(
    conn: &Connection,
    assistente_id: &str,
    stato: &str,
    posto: Option<&Posto>,
    errore: Option<&str>,
) -> Risposta<()> {
    let ora = ora_ms();
    let acceso_ms = (stato == "acceso").then_some(ora);
    conn.execute(
        "INSERT INTO computer_assistente (assistente_id, stato, display, porta_vnc, porta_ws,
           porta_ttyd, porta_chrome, acceso_ms, errore, aggiornato_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(assistente_id) DO UPDATE SET stato = excluded.stato,
           display = COALESCE(excluded.display, display),
           porta_vnc = COALESCE(excluded.porta_vnc, porta_vnc),
           porta_ws = COALESCE(excluded.porta_ws, porta_ws),
           porta_ttyd = COALESCE(excluded.porta_ttyd, porta_ttyd),
           porta_chrome = COALESCE(excluded.porta_chrome, porta_chrome),
           acceso_ms = CASE WHEN excluded.stato = 'acceso' THEN excluded.acceso_ms
                            WHEN excluded.stato = 'avvio' THEN acceso_ms ELSE NULL END,
           errore = excluded.errore, aggiornato_ms = excluded.aggiornato_ms",
        params![
            assistente_id,
            stato,
            posto.map(|p| p.display),
            posto.map(|p| p.porta_vnc),
            posto.map(|p| p.porta_ws),
            posto.map(|p| p.porta_ttyd),
            posto.map(|p| p.porta_chrome),
            acceso_ms,
            errore,
            ora
        ],
    )
    .db()?;
    Ok(())
}

/// Spento: il posto si libera (display e porte a NULL).
pub fn libera(conn: &Connection, assistente_id: &str) -> Risposta<()> {
    conn.execute(
        "UPDATE computer_assistente SET stato = 'spento', display = NULL, porta_vnc = NULL,
           porta_ws = NULL, porta_ttyd = NULL, porta_chrome = NULL, acceso_ms = NULL,
           errore = NULL, aggiornato_ms = ?2 WHERE assistente_id = ?1",
        params![assistente_id, ora_ms()],
    )
    .db()?;
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::prepara(&c).unwrap();
        c
    }

    fn assistente(c: &Connection, nome: &str) -> String {
        let n = crate::assistenti::NuovoAssistente {
            nome: nome.into(),
            ..Default::default()
        };
        crate::assistenti::crea(c, &n).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn posti_volonta_e_stato() {
        let c = db();
        let a = assistente(&c, "Uno");
        let b = assistente(&c, "Due");
        assert_eq!(stato(&c, &a).unwrap(), "spento");
        assert_eq!(leggi(&c, &a).unwrap()["stato"], "spento");
        let pa = assegna_posto(&c, &a, |_| true).unwrap().unwrap();
        assert_eq!(pa, Posto::numero(0));
        scrivi(&c, &a, "avvio", Some(&pa), None).unwrap();
        // Il posto 1 risulta occupato da altro: si salta.
        let pb = assegna_posto(&c, &b, |p| p.display != 51).unwrap().unwrap();
        assert_eq!(pb.display, 52);
        assert_eq!(assegna_posto(&c, &a, |_| true).unwrap(), Some(pa));
        assert_eq!(assegna_posto(&c, &a, |_| false).unwrap(), None);
        scrivi(&c, &a, "acceso", None, None).unwrap();
        let r = leggi(&c, &a).unwrap();
        assert_eq!(r["stato"], "acceso");
        assert_eq!(r["display"], 50);
        assert!(r["acceso_ms"].as_i64().is_some());
        scrivi(&c, &a, "errore", None, Some("Xvfb terminato")).unwrap();
        assert_eq!(leggi(&c, &a).unwrap()["errore"], "Xvfb terminato");
        imposta_acceso(&c, &a, true).unwrap();
        assert_eq!(da_accendere(&c).unwrap(), vec![a.clone()]);
        let x = crate::assistenti::leggi(&c, &a).unwrap();
        assert_eq!(x["computer"]["stato"], "errore");
        assert_eq!(x["computer_acceso"], 1);
        libera(&c, &a).unwrap();
        assert_eq!(leggi(&c, &a).unwrap()["display"], Value::Null);
    }
}
