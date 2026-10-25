//! Blob (file sul disco, indirizzati per sha256) e allegati delle sessioni.
//!
//! Un blob sta in `<cartella>/<prime due cifre>/<hash>`; la riga `blob` ne tiene dimensione,
//! tipo e percorso. Lo stesso contenuto caricato due volte è un solo blob.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use xbot_core::{Codice, Errore, Risposta, nuovo_id, ora_ms};

use crate::util::{RisultatoDb, tutte_json, una_json};

/// Un hash di blob valido: 64 cifre esadecimali minuscole.
pub fn hash_valido(h: &str) -> bool {
    h.len() == 64
        && h.chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

pub fn hash_di(byte: &[u8]) -> String {
    Sha256::digest(byte)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn io(e: std::io::Error) -> Errore {
    Errore::interno(format!("file: {e}"))
}

/// Scrive il contenuto nella cartella dei blob (se non c'è già) e registra la riga.
pub fn salva_blob(
    conn: &Connection,
    cartella: &Path,
    byte: &[u8],
    tipo_mime: Option<&str>,
) -> Risposta<(String, PathBuf)> {
    let hash = hash_di(byte);
    let dir = cartella.join(&hash[..2]);
    let percorso = dir.join(&hash);
    if !percorso.is_file() {
        std::fs::create_dir_all(&dir).map_err(io)?;
        let provvisorio = dir.join(format!("{hash}.{}", nuovo_id()));
        std::fs::write(&provvisorio, byte).map_err(io)?;
        std::fs::rename(&provvisorio, &percorso).map_err(io)?;
    }
    let dimensione = i64::try_from(byte.len()).unwrap_or(i64::MAX);
    conn.execute(
        "INSERT INTO blob (hash, dimensione, tipo_mime, percorso, creato_ms)
         VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(hash) DO NOTHING",
        params![
            hash,
            dimensione,
            tipo_mime,
            percorso.to_string_lossy(),
            ora_ms()
        ],
    )
    .db()?;
    Ok((hash, percorso))
}

/// Il blob: (percorso, tipo, dimensione).
pub fn blob(conn: &Connection, hash: &str) -> Risposta<(PathBuf, Option<String>, i64)> {
    if !hash_valido(hash) {
        return Err(Errore::nuovo(Codice::NonTrovato, "blob non trovato"));
    }
    conn.query_row(
        "SELECT percorso, tipo_mime, dimensione FROM blob WHERE hash = ?1",
        [hash],
        |r| Ok((PathBuf::from(r.get::<_, String>(0)?), r.get(1)?, r.get(2)?)),
    )
    .optional()
    .db()?
    .ok_or_else(|| Errore::nuovo(Codice::NonTrovato, "blob non trovato"))
}

/// Un nome di file senza cartelle né caratteri strani (al massimo 120 caratteri).
pub fn nome_pulito(nome: &str) -> String {
    let base = nome.rsplit(['/', '\\']).next().unwrap_or("");
    let s: String = base
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .take(120)
        .collect();
    let s = s.trim().trim_start_matches('.').to_string();
    if s.is_empty() { "file".into() } else { s }
}

/// Registra un allegato della sessione: `{id, nome, blob_hash}`.
pub fn crea_allegato(
    conn: &Connection,
    sessione_id: &str,
    nome: &str,
    blob_hash: &str,
) -> Risposta<Value> {
    crate::sessioni::leggi(conn, sessione_id)?;
    let id = nuovo_id();
    let nome = nome_pulito(nome);
    conn.execute(
        "INSERT INTO allegato (id, sessione_id, nome, blob_hash, caricato_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, sessione_id, nome, blob_hash, ora_ms()],
    )
    .db()?;
    Ok(json!({ "id": id, "nome": nome, "blob_hash": blob_hash }))
}

/// Un allegato con il percorso del suo blob, solo se è della sessione data.
pub fn allegato_della_sessione(
    conn: &Connection,
    sessione_id: &str,
    id: &str,
) -> Risposta<Option<(String, PathBuf)>> {
    conn.query_row(
        "SELECT a.nome, b.percorso FROM allegato a JOIN blob b ON b.hash = a.blob_hash
         WHERE a.id = ?1 AND a.sessione_id = ?2",
        params![id, sessione_id],
        |r| Ok((r.get(0)?, PathBuf::from(r.get::<_, String>(1)?))),
    )
    .optional()
    .db()
}

/// La Libreria dell'assistente: gli allegati delle sue sessioni e i blob delle sue righe.
pub fn dell_assistente(conn: &Connection, assistente_id: &str) -> Risposta<Vec<Value>> {
    crate::assistenti::leggi(conn, assistente_id)?;
    tutte_json(
        conn,
        "SELECT a.id, a.nome, a.blob_hash, b.dimensione, b.tipo_mime, a.caricato_ms, a.sessione_id
         FROM allegato a JOIN blob b ON b.hash = a.blob_hash JOIN sessione s ON s.id = a.sessione_id
         WHERE s.assistente_id = ?1
         UNION ALL
         SELECT r.id, COALESCE(json_extract(r.corpo_json, '$.nome'), r.genere), r.blob_hash,
           b.dimensione, b.tipo_mime, r.creata_ms, r.sessione_id
         FROM riga r JOIN blob b ON b.hash = r.blob_hash JOIN sessione s ON s.id = r.sessione_id
         WHERE s.assistente_id = ?1
         ORDER BY 6 DESC LIMIT 500",
        [assistente_id],
    )
}

pub fn leggi(conn: &Connection, id: &str) -> Risposta<Value> {
    una_json(conn, "SELECT * FROM allegato WHERE id = ?1", [id])?
        .ok_or_else(|| Errore::nuovo(Codice::NonTrovato, "allegato non trovato"))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::assistenti::{NuovoAssistente, crea};
    use crate::sessioni::{self, NuovaSessione};

    #[test]
    fn blob_unico_e_allegato() {
        let db = crate::db_prova();
        let dir = std::env::temp_dir().join(format!("xbot-blob-{}", nuovo_id()));
        db.con(|c| {
            let (h1, p1) = salva_blob(c, &dir, b"ciao", Some("text/plain"))?;
            let (h2, _) = salva_blob(c, &dir, b"ciao", None)?;
            assert_eq!(h1, h2);
            assert!(hash_valido(&h1));
            assert_eq!(std::fs::read(&p1).unwrap(), b"ciao");
            assert_eq!(blob(c, &h1)?.2, 4);
            assert!(blob(c, "../x").is_err());
            let a = crea(
                c,
                &NuovoAssistente {
                    nome: "J".into(),
                    ..Default::default()
                },
            )?;
            let aid = a["id"].as_str().unwrap();
            let s = sessioni::crea(c, aid, &NuovaSessione::default())?;
            let al = crea_allegato(c, &s.id, "../../etc/passwd", &h1)?;
            assert_eq!(al["nome"], "passwd");
            let id = al["id"].as_str().unwrap();
            assert!(allegato_della_sessione(c, &s.id, id)?.is_some());
            assert!(allegato_della_sessione(c, "altra", id)?.is_none());
            assert_eq!(dell_assistente(c, aid)?.len(), 1);
            Ok(())
        })
        .unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
