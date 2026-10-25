//! Invii dell'utente: idempotenti per id (il nonce del client).

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use serde_json::Value;
use xbot_core::{Codice, Consegna, Errore, Id, Ms, Risposta, StatoInvio, ora_ms};

use crate::util::RisultatoDb;

#[derive(Debug, Clone, Serialize)]
pub struct Invio {
    pub id: Id,
    pub sessione_id: Id,
    pub testo: String,
    pub allegati: Value,
    pub risposta_a_riga_id: Option<String>,
    pub origine: String,
    pub macchina_destinazione_id: Option<String>,
    pub provider_richiesto_id: Option<String>,
    pub inviato_ms: Ms,
    pub stato: String,
    pub consegna: Option<String>,
    pub rifiuto_codice: Option<String>,
    pub rifiuto_testo: Option<String>,
    pub riga_eco_id: Option<String>,
    pub accettato_ms: Option<Ms>,
}

const COLONNE: &str = "id, sessione_id, testo, allegati_json, risposta_a_riga_id, origine, \
    macchina_destinazione_id, provider_richiesto_id, inviato_ms, stato, consegna, rifiuto_codice, \
    rifiuto_testo, riga_eco_id, accettato_ms";

fn da_riga(r: &Row<'_>) -> rusqlite::Result<Invio> {
    let allegati: String = r.get(3)?;
    Ok(Invio {
        id: r.get(0)?,
        sessione_id: r.get(1)?,
        testo: r.get(2)?,
        allegati: serde_json::from_str(&allegati).unwrap_or(Value::Array(vec![])),
        risposta_a_riga_id: r.get(4)?,
        origine: r.get(5)?,
        macchina_destinazione_id: r.get(6)?,
        provider_richiesto_id: r.get(7)?,
        inviato_ms: r.get(8)?,
        stato: r.get(9)?,
        consegna: r.get(10)?,
        rifiuto_codice: r.get(11)?,
        rifiuto_testo: r.get(12)?,
        riga_eco_id: r.get(13)?,
        accettato_ms: r.get(14)?,
    })
}

#[derive(Debug, Clone)]
pub struct NuovoInvio {
    pub id: Id,
    pub sessione_id: Id,
    pub testo: String,
    pub allegati: Value,
    pub risposta_a_riga_id: Option<String>,
    pub origine: String,
    pub macchina_destinazione_id: Option<String>,
    pub provider_richiesto_id: Option<String>,
    pub composto_ms: Option<Ms>,
}

/// L'esito di `crea`: nuovo, oppure l'invio che esisteva già con quell'id.
#[derive(Debug, Clone)]
pub enum EsitoCrea {
    Nuovo(Invio),
    Duplicato(Invio),
}

/// Crea l'invio in stato `in_coda`. Se l'id esiste già non crea nulla: `Duplicato`.
pub fn crea(conn: &Connection, n: &NuovoInvio) -> Risposta<EsitoCrea> {
    if let Some(esistente) = leggi_opzionale(conn, &n.id)? {
        return Ok(EsitoCrea::Duplicato(esistente));
    }
    conn.execute(
        "INSERT INTO invio (id, sessione_id, testo, allegati_json, risposta_a_riga_id, origine,
           macchina_destinazione_id, provider_richiesto_id, composto_ms, inviato_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            n.id,
            n.sessione_id,
            n.testo,
            n.allegati.to_string(),
            n.risposta_a_riga_id,
            n.origine,
            n.macchina_destinazione_id,
            n.provider_richiesto_id,
            n.composto_ms,
            ora_ms()
        ],
    )
    .db()?;
    Ok(EsitoCrea::Nuovo(leggi(conn, &n.id)?))
}

fn leggi_opzionale(conn: &Connection, id: &str) -> Risposta<Option<Invio>> {
    conn.query_row(
        &format!("SELECT {COLONNE} FROM invio WHERE id = ?1"),
        [id],
        da_riga,
    )
    .optional()
    .db()
}

pub fn leggi(conn: &Connection, id: &str) -> Risposta<Invio> {
    leggi_opzionale(conn, id)?.ok_or_else(|| Errore::nuovo(Codice::NonTrovato, "invio non trovato"))
}

/// Accettato: con la consegna scelta e la riga d'eco creata.
pub fn accetta(
    conn: &Connection,
    id: &str,
    consegna: Consegna,
    riga_eco_id: Option<&str>,
) -> Risposta<Invio> {
    conn.execute(
        "UPDATE invio SET stato = 'accettato', consegna = ?2, riga_eco_id = ?3, accettato_ms = ?4
         WHERE id = ?1",
        params![id, consegna.come_testo(), riga_eco_id, ora_ms()],
    )
    .db()?;
    leggi(conn, id)
}

/// Rifiutato: con codice e testo, nessuna riga creata.
pub fn rifiuta(conn: &Connection, id: &str, errore: &Errore) -> Risposta<Invio> {
    conn.execute(
        "UPDATE invio SET stato = 'rifiutato', rifiuto_codice = ?2, rifiuto_testo = ?3 WHERE id = ?1",
        params![id, errore.codice.come_testo(), errore.testo],
    )
    .db()?;
    leggi(conn, id)
}

pub fn imposta_stato(conn: &Connection, id: &str, stato: StatoInvio) -> Risposta<()> {
    conn.execute(
        "UPDATE invio SET stato = ?2 WHERE id = ?1",
        params![id, stato.come_testo()],
    )
    .db()?;
    Ok(())
}

pub fn imposta_consegna(conn: &Connection, id: &str, consegna: Consegna) -> Risposta<()> {
    conn.execute(
        "UPDATE invio SET consegna = ?2 WHERE id = ?1",
        params![id, consegna.come_testo()],
    )
    .db()?;
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::sessioni;
    use serde_json::json;

    #[test]
    fn stesso_id_e_duplicato_senza_creare_nulla() {
        let db = crate::db_prova();
        db.con(|c| {
            let s = sessioni::test::sessione_prova(c);
            let n = NuovoInvio {
                id: xbot_core::nuovo_id(),
                sessione_id: s.id.clone(),
                testo: "ciao".into(),
                allegati: json!([]),
                risposta_a_riga_id: None,
                origine: "pwa".into(),
                macchina_destinazione_id: None,
                provider_richiesto_id: None,
                composto_ms: None,
            };
            let EsitoCrea::Nuovo(i) = crea(c, &n)? else {
                panic!("doveva essere nuovo")
            };
            assert_eq!(i.stato, "in_coda");
            accetta(c, &i.id, Consegna::AvviatoContinuazione, None)?;
            let EsitoCrea::Duplicato(d) = crea(c, &n)? else {
                panic!("doveva essere duplicato")
            };
            assert_eq!(d.stato, "accettato");
            let quanti: i64 = c
                .query_row("SELECT COUNT(*) FROM invio", [], |r| r.get(0))
                .db()?;
            assert_eq!(quanti, 1);
            let r = rifiuta(c, &i.id, &Errore::nuovo(Codice::ProviderFermo, "fermo"))?;
            assert_eq!(r.rifiuto_codice.as_deref(), Some("provider_fermo"));
            Ok(())
        })
        .unwrap();
    }
}
