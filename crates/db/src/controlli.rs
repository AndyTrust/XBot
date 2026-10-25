//! Controlli prima di scrivere: riferimenti che devono esistere, assistenti attivi, lunghezze.
//! Danno codici dedicati invece dell'errore di vincolo di SQLite.

use rusqlite::{Connection, OptionalExtension};
use serde_json::{Map, Value};
use xbot_core::{Codice, Errore, Risposta};

use crate::util::RisultatoDb;

/// Lunghezze massime, in caratteri.
pub const NOME_ASSISTENTE: usize = 80;
pub const TITOLO_ASSISTENTE: usize = 120;
pub const DESCRIZIONE: usize = 2000;
pub const TITOLO_SESSIONE: usize = 200;
pub const TESTO_INVIO: usize = 100_000;
pub const CHIAVE_IMPOSTAZIONE: usize = 80;

/// `testo_non_valido` se il testo contiene caratteri di controllo: nei nomi (`riga_sola`)
/// anche a capo e tabulazioni, negli altri testi solo quelli diversi da `\n`, `\r`, `\t`
/// (un NUL rompe l'argomento di `claude`).
pub fn testo_valido(campo: &str, testo: Option<&str>, riga_sola: bool) -> Risposta<()> {
    let cattivo = |c: char| c.is_control() && (riga_sola || !matches!(c, '\n' | '\r' | '\t'));
    match testo {
        Some(t) if t.chars().any(cattivo) => Err(Errore::nuovo(
            Codice::TestoNonValido,
            format!("{campo}: caratteri di controllo non ammessi"),
        )),
        _ => Ok(()),
    }
}

/// I nomi (una riga sola) presenti in una modifica parziale.
pub fn nomi_validi(campi: &Map<String, Value>, nomi: &[&str]) -> Risposta<()> {
    for n in nomi {
        testo_valido(n, campi.get(*n).and_then(Value::as_str), true)?;
    }
    Ok(())
}

/// `testo_troppo_lungo` se `testo` supera `max` caratteri; `testo_non_valido` con caratteri
/// di controllo diversi da a capo e tabulazione.
pub fn lunghezza(campo: &str, testo: Option<&str>, max: usize) -> Risposta<()> {
    testo_valido(campo, testo, false)?;
    match testo {
        Some(t) if t.chars().count() > max => Err(Errore::nuovo(
            Codice::TestoTroppoLungo,
            format!("{campo}: al massimo {max} caratteri"),
        )),
        _ => Ok(()),
    }
}

/// Le lunghezze dei campi presenti in una modifica parziale: (campo, massimo).
pub fn lunghezze(campi: &Map<String, Value>, limiti: &[(&str, usize)]) -> Risposta<()> {
    for (campo, max) in limiti {
        lunghezza(campo, campi.get(*campo).and_then(Value::as_str), *max)?;
    }
    Ok(())
}

fn esiste(conn: &Connection, tabella: &str, id: &str) -> Risposta<bool> {
    // `tabella` viene sempre da questo modulo, mai dal client.
    let sql = format!("SELECT 1 FROM {tabella} WHERE id = ?1");
    Ok(conn
        .query_row(&sql, [id], |_| Ok(()))
        .optional()
        .db()?
        .is_some())
}

pub fn provider(conn: &Connection, id: &str) -> Risposta<()> {
    if esiste(conn, "provider", id)? {
        Ok(())
    } else {
        Err(Errore::nuovo(
            Codice::ProviderNonTrovato,
            format!("il provider {id} non esiste"),
        ))
    }
}

pub fn modello(conn: &Connection, id: &str) -> Risposta<()> {
    if esiste(conn, "modello", id)? {
        Ok(())
    } else {
        Err(Errore::nuovo(
            Codice::ModelloNonDisponibile,
            format!("il modello {id} non esiste"),
        ))
    }
}

pub fn sezione(conn: &Connection, id: &str) -> Risposta<()> {
    if esiste(conn, "sezione_barra", id)? {
        Ok(())
    } else {
        Err(Errore::nuovo(
            Codice::SezioneNonTrovata,
            format!("la sezione {id} non esiste"),
        ))
    }
}

fn altro(conn: &Connection, tabella: &str, campo: &str, id: &str) -> Risposta<()> {
    if esiste(conn, tabella, id)? {
        Ok(())
    } else {
        Err(Errore::nuovo(
            Codice::RiferimentoNonTrovato,
            format!("{campo}: {id} non esiste"),
        ))
    }
}

/// Controlla un riferimento per nome di campo; i campi che non sono riferimenti passano.
pub fn riferimento(conn: &Connection, campo: &str, id: Option<&str>) -> Risposta<()> {
    let Some(id) = id.filter(|i| !i.is_empty()) else {
        return Ok(());
    };
    match campo {
        "provider_id" | "provider_predefinito_id" | "provider" => provider(conn, id),
        "modello_id" | "modello_predefinito_id" => modello(conn, id),
        "sezione_id" => sezione(conn, id),
        "macchina_id" | "macchina" | "macchina_predefinita_id" => {
            altro(conn, "macchina", campo, id)
        }
        "progetto_id" => altro(conn, "progetto", campo, id),
        _ => Ok(()),
    }
}

/// Tutti i riferimenti di una modifica parziale.
pub fn riferimenti(conn: &Connection, campi: &Map<String, Value>) -> Risposta<()> {
    for (k, v) in campi {
        riferimento(conn, k, v.as_str())?;
    }
    Ok(())
}

/// L'assistente esiste ed è attivo: altrimenti 404 `assistente_non_trovato`
/// o 409 `assistente_archiviato`.
pub fn assistente_attivo(conn: &Connection, id: &str) -> Risposta<()> {
    let archiviato: Option<Option<i64>> = conn
        .query_row(
            "SELECT archiviato_ms FROM assistente WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()
        .db()?;
    match archiviato {
        None => Err(Errore::nuovo(
            Codice::AssistenteNonTrovato,
            "assistente non trovato",
        )),
        Some(Some(_)) => Err(Errore::nuovo(
            Codice::AssistenteArchiviato,
            "l'assistente è archiviato",
        )),
        Some(None) => Ok(()),
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json::json;

    #[test]
    fn riferimenti_e_lunghezze() {
        let db = crate::db_prova();
        db.con(|c| {
            let codice = |r: Risposta<()>| r.unwrap_err().codice;
            assert_eq!(codice(provider(c, "no")), Codice::ProviderNonTrovato);
            assert_eq!(codice(modello(c, "no")), Codice::ModelloNonDisponibile);
            assert_eq!(codice(sezione(c, "no")), Codice::SezioneNonTrovata);
            assert!(sezione(c, crate::assistenti::NON_ASSEGNATI).is_ok());
            let m = json!({ "macchina_id": "no", "titolo": "x" });
            assert_eq!(
                codice(riferimenti(c, m.as_object().unwrap())),
                Codice::RiferimentoNonTrovato
            );
            assert!(riferimento(c, "titolo", Some("qualsiasi")).is_ok());
            assert_eq!(
                codice(assistente_attivo(c, "no")),
                Codice::AssistenteNonTrovato
            );
            assert!(lunghezza("nome", Some("àèì"), 3).is_ok());
            assert_eq!(
                codice(lunghezza("nome", Some("àèìò"), 3)),
                Codice::TestoTroppoLungo
            );
            Ok(())
        })
        .unwrap();
    }
}
