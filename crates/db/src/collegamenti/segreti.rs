//! Intestazioni e ambiente dei collegamenti: cifrati con la chiave della cassaforte, nomi in chiaro.

use rusqlite::Connection;
use serde_json::{Map, Value, json};
use xbot_core::{Codice, Errore, Risposta, SegretoTesto};

use crate::cassaforte::{self, Chiave};
use crate::util::RisultatoDb;

pub(super) fn aad(id: &str, campo: &str) -> String {
    format!("collegamento:{id}:{campo}")
}

pub(super) fn senza_chiave() -> Errore {
    Errore::nuovo(
        Codice::CassaforteNonImpostata,
        "la cassaforte è chiusa: intestazioni e ambiente non si possono cifrare",
    )
}

/// Le coppie nome→valore (tutte stringhe) cifrate in un BLOB.
pub(super) fn cifra_mappa(
    chiave: Option<&Chiave>,
    id: &str,
    campo: &str,
    m: &Map<String, Value>,
) -> Risposta<Option<Vec<u8>>> {
    if m.is_empty() {
        return Ok(None);
    }
    for (k, v) in m {
        if k.trim().is_empty() || !v.is_string() {
            return Err(Errore::non_valida(format!(
                "{campo}: ogni voce vuole un nome e un testo"
            )));
        }
    }
    let chiave = chiave.ok_or_else(senza_chiave)?;
    let testo = SegretoTesto::nuovo(Value::Object(m.clone()).to_string());
    cassaforte::cifra(chiave, &aad(id, campo), testo.esponi().as_bytes()).map(Some)
}

/// I nomi di una mappa, in JSON (le colonne `*_nomi_json`, in chiaro).
pub(super) fn nomi_json(m: &Map<String, Value>) -> String {
    json!(m.keys().collect::<Vec<_>>()).to_string()
}

pub(super) fn decifra_mappa(
    chiave: Option<&Chiave>,
    id: &str,
    campo: &str,
    blob: Option<Vec<u8>>,
) -> Risposta<Vec<(String, SegretoTesto)>> {
    let Some(blob) = blob else {
        return Ok(Vec::new());
    };
    let chiave = chiave.ok_or_else(senza_chiave)?;
    let chiaro = SegretoTesto::from(
        String::from_utf8(cassaforte::decifra(chiave, &aad(id, campo), &blob)?)
            .map_err(|_| Errore::nuovo(Codice::Cifratura, "campi non in UTF-8"))?,
    );
    let m: Map<String, Value> = serde_json::from_str(chiaro.esponi())
        .map_err(|_| Errore::nuovo(Codice::Cifratura, "campi illeggibili"))?;
    Ok(m.into_iter()
        .map(|(k, v)| (k, SegretoTesto::nuovo(v.as_str().unwrap_or_default())))
        .collect())
}

/// La mappa salvata fusa con quella del PATCH (`null` = cancella).
pub(super) fn fondi(
    conn: &Connection,
    chiave: Option<&Chiave>,
    id: &str,
    campo: &str,
    col: &str,
    v: &Value,
) -> Risposta<Map<String, Value>> {
    let Some(nuova) = v.as_object() else {
        if v.is_null() {
            return Ok(Map::new());
        }
        return Err(Errore::non_valida(format!(
            "{campo}: una mappa nome → testo"
        )));
    };
    let blob: Option<Vec<u8>> = conn
        .query_row(
            &format!("SELECT {col} FROM collegamento WHERE id = ?1"),
            [id],
            |r| r.get(0),
        )
        .db()?;
    let mut m: Map<String, Value> = if blob.is_some() {
        decifra_mappa(chiave, id, campo, blob)?
            .into_iter()
            .map(|(k, s)| (k, Value::String(s.esponi().to_string())))
            .collect()
    } else {
        Map::new()
    };
    for (k, val) in nuova {
        if val.is_null() {
            m.remove(k);
        } else {
            m.insert(k.clone(), val.clone());
        }
    }
    Ok(m)
}
