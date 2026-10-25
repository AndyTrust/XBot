//! Assistenti: creazione, elenco, modifica, archiviazione. Le risposte sono oggetti JSON.

use rusqlite::{Connection, params};
use serde::Deserialize;
use serde_json::{Map, Value};
use xbot_core::{Codice, Errore, Risposta, nuovo_id, ora_ms};

use crate::controlli;
use crate::util::{RisultatoDb, aggiorna_campi, in_transazione, tutte_json, una_json};

/// La sezione speciale della barra per chi non ha sezione.
pub const NON_ASSEGNATI: &str = "__non_assegnati__";

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NuovoAssistente {
    pub nome: String,
    pub descrizione: Option<String>,
    pub titolo: Option<String>,
    pub avatar_forma: Option<String>,
    pub avatar_colore: Option<String>,
    pub profilo_percorso: Option<String>,
    pub memoria_cartella: Option<String>,
    pub quaderno_percorso: Option<String>,
    pub provider_predefinito_id: Option<String>,
    pub modello_predefinito_id: Option<String>,
    // --- S-B (2.E) ---
    #[serde(default)]
    pub macchina_predefinita_id: Option<String>,
    // --- fine S-B ---
    pub livello_ragionamento: Option<String>,
    pub sezione_id: Option<String>,
}

const SELEZIONE: &str = "SELECT a.*, sa.sezione_id FROM assistente a
    LEFT JOIN sezione_assistente sa ON sa.assistente_id = a.id";

pub fn crea(conn: &Connection, n: &NuovoAssistente) -> Risposta<Value> {
    if n.nome.trim().is_empty() {
        return Err(Errore::non_valida("il nome è obbligatorio"));
    }
    controlli::testo_valido("nome", Some(&n.nome), true)?;
    controlli::testo_valido("titolo", n.titolo.as_deref(), true)?;
    controlli::lunghezza("nome", Some(&n.nome), controlli::NOME_ASSISTENTE)?;
    controlli::lunghezza("titolo", n.titolo.as_deref(), controlli::TITOLO_ASSISTENTE)?;
    controlli::lunghezza(
        "descrizione",
        n.descrizione.as_deref(),
        controlli::DESCRIZIONE,
    )?;
    for (campo, valore) in [
        ("provider_predefinito_id", &n.provider_predefinito_id),
        ("modello_predefinito_id", &n.modello_predefinito_id),
        ("macchina_predefinita_id", &n.macchina_predefinita_id),
        ("sezione_id", &n.sezione_id),
    ] {
        controlli::riferimento(conn, campo, valore.as_deref())?;
    }
    let id = nuovo_id();
    let ora = ora_ms();
    let sezione = n.sezione_id.clone().unwrap_or_else(|| NON_ASSEGNATI.into());
    in_transazione(conn, || {
        conn.execute(
            "INSERT INTO assistente (id, nome, descrizione, titolo, avatar_forma, avatar_colore,
               profilo_percorso, memoria_cartella, quaderno_percorso, provider_predefinito_id,
               modello_predefinito_id, livello_ragionamento, creato_ms, aggiornato_ms,
               macchina_predefinita_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13, ?14)",
            params![
                id,
                n.nome.trim(),
                n.descrizione,
                n.titolo,
                n.avatar_forma,
                n.avatar_colore,
                n.profilo_percorso,
                n.memoria_cartella,
                n.quaderno_percorso,
                n.provider_predefinito_id,
                n.modello_predefinito_id,
                n.livello_ragionamento,
                ora,
                n.macchina_predefinita_id
            ],
        )
        .db()?;
        sposta_in_sezione(conn, &id, &sezione, None)?;
        conn.execute(
            "INSERT INTO stato_lettura (assistente_id) VALUES (?1) ON CONFLICT DO NOTHING",
            [&id],
        )
        .db()?;
        leggi(conn, &id)
    })
}

pub fn leggi(conn: &Connection, id: &str) -> Risposta<Value> {
    let mut a = una_json(conn, &format!("{SELEZIONE} WHERE a.id = ?1"), [id])?
        .ok_or_else(|| Errore::nuovo(Codice::AssistenteNonTrovato, "assistente non trovato"))?;
    arricchisci(conn, std::slice::from_mut(&mut a))?;
    Ok(a)
}

/// Gli assistenti in ordine di nome; `archiviati` include gli archiviati.
pub fn elenca(conn: &Connection, archiviati: bool) -> Risposta<Vec<Value>> {
    let filtro = if archiviati {
        ""
    } else {
        "WHERE a.archiviato_ms IS NULL"
    };
    let mut v = tutte_json(
        conn,
        &format!("{SELEZIONE} {filtro} ORDER BY a.fissato DESC, a.ordine_fissato, a.nome"),
        [],
    )?;
    arricchisci(conn, &mut v)?;
    Ok(v)
}

// --- S1 (Fase 2.D): menzione e avatar ---

/// Il nome come `@menzione`: minuscolo, senza spazi né segni (lettere, cifre, `_`, `-`).
pub fn menzione_base(nome: &str) -> String {
    let s: String = nome
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric() || matches!(c, '_' | '-'))
        .collect();
    if s.is_empty() { "assistente".into() } else { s }
}

/// Le menzioni di tutti gli assistenti non archiviati, uniche: a parità di nome il più
/// vecchio tiene la base, gli altri prendono un suffisso numerico (`atena`, `atena2`, …).
pub fn menzioni(conn: &Connection) -> Risposta<Vec<(String, String)>> {
    let mut st = conn
        .prepare(
            "SELECT id, nome FROM assistente WHERE archiviato_ms IS NULL ORDER BY creato_ms, id",
        )
        .db()?;
    let righe = st
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .db()?
        .collect::<rusqlite::Result<Vec<_>>>()
        .db()?;
    let basi: Vec<String> = righe.iter().map(|(_, n)| menzione_base(n)).collect();
    let mut prese = std::collections::HashSet::new();
    let mut fuori = Vec::new();
    for ((id, _), base) in righe.into_iter().zip(&basi) {
        let mut m = base.clone();
        let mut n = 2;
        while prese.contains(&m) || (m != *base && basi.contains(&m)) {
            m = format!("{base}{n}");
            n += 1;
        }
        prese.insert(m.clone());
        fuori.push((id, m));
    }
    Ok(fuori)
}

/// L'assistente (non archiviato) con questa menzione, senza `@` e senza badare alle maiuscole.
pub fn trova_per_menzione(conn: &Connection, menzione: &str) -> Risposta<Option<String>> {
    let cercata = menzione.trim_start_matches('@').to_lowercase();
    Ok(menzioni(conn)?
        .into_iter()
        .find(|(_, m)| *m == cercata)
        .map(|(id, _)| id))
}

/// Aggiunge `menzione` e `avatar_url` (da `avatar_percorso`, l'hash del blob della foto).
fn arricchisci(conn: &Connection, v: &mut [Value]) -> Risposta<()> {
    let m: std::collections::HashMap<String, String> = menzioni(conn)?.into_iter().collect();
    for a in v {
        let id = a["id"].as_str().unwrap_or("").to_string();
        let menzione = m
            .get(&id)
            .cloned()
            .unwrap_or_else(|| menzione_base(a["nome"].as_str().unwrap_or("")));
        a["menzione"] = Value::from(menzione);
        a["avatar_url"] = match a["avatar_percorso"].as_str() {
            Some(h) if crate::allegati::hash_valido(h) => Value::from(format!("/api/v1/blob/{h}")),
            _ => Value::Null,
        };
        // --- 2.H --- lo stato del computer dell'assistente.
        a["computer"] = serde_json::json!({ "stato": crate::computer::stato(conn, &id)? });
    }
    Ok(())
}
// --- fine S1 ---

const MODIFICABILI: &[&str] = &[
    "nome",
    "descrizione",
    "titolo",
    "avatar_forma",
    "avatar_colore",
    "avatar_percorso",
    "profilo_percorso",
    "memoria_cartella",
    "quaderno_percorso",
    "provider_predefinito_id",
    "modello_predefinito_id",
    "macchina_predefinita_id",
    "usa_chrome",
    "livello_ragionamento",
    "voce_id",
    "voce_velocita",
    "voce_lingua",
    "colore_in_chat",
    "fissato",
    "nascosto",
    "ordine_fissato",
];

/// Modifica parziale. `sezione_id` (e `ordine_in_sezione`) spostano l'assistente nella barra.
pub fn aggiorna(conn: &Connection, id: &str, campi: &Map<String, Value>) -> Risposta<Value> {
    leggi(conn, id)?;
    controlli::riferimenti(conn, campi)?;
    controlli::nomi_validi(campi, &["nome", "titolo"])?;
    controlli::lunghezze(
        campi,
        &[
            ("nome", controlli::NOME_ASSISTENTE),
            ("titolo", controlli::TITOLO_ASSISTENTE),
            ("descrizione", controlli::DESCRIZIONE),
        ],
    )?;
    let mut campi = campi.clone();
    let sezione = campi.remove("sezione_id");
    let ordine = campi.remove("ordine_in_sezione").and_then(|v| v.as_i64());
    in_transazione(conn, || {
        aggiorna_campi(
            conn,
            "assistente",
            "id",
            id,
            &campi,
            MODIFICABILI,
            &[("aggiornato_ms", Value::from(ora_ms()))],
        )?;
        match sezione {
            Some(Value::String(s)) => sposta_in_sezione(conn, id, &s, ordine)?,
            Some(Value::Null) => sposta_in_sezione(conn, id, NON_ASSEGNATI, ordine)?,
            Some(_) => return Err(Errore::non_valida("sezione_id vuole un testo")),
            None => {}
        }
        leggi(conn, id)
    })
}

/// Un assistente sta in una sola sezione della barra.
fn sposta_in_sezione(
    conn: &Connection,
    id: &str,
    sezione: &str,
    ordine: Option<i64>,
) -> Risposta<()> {
    conn.execute(
        "DELETE FROM sezione_assistente WHERE assistente_id = ?1",
        [id],
    )
    .db()?;
    conn.execute(
        "INSERT INTO sezione_assistente (sezione_id, assistente_id, ordine) VALUES (?1, ?2, ?3)",
        params![sezione, id, ordine.unwrap_or(0)],
    )
    .db()?;
    Ok(())
}

/// DELETE = archivia: l'assistente esce dalla barra ma resta nel database.
pub fn archivia(conn: &Connection, id: &str) -> Risposta<Value> {
    let ora = ora_ms();
    let n = conn
        .execute(
            "UPDATE assistente SET archiviato_ms = ?2, aggiornato_ms = ?2
             WHERE id = ?1 AND archiviato_ms IS NULL",
            params![id, ora],
        )
        .db()?;
    if n == 0 {
        leggi(conn, id)?;
    }
    leggi(conn, id)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn crea_sposta_archivia() {
        let db = crate::db_prova();
        db.con(|c| {
            let a = crea(
                c,
                &NuovoAssistente {
                    nome: "Atena".into(),
                    ..Default::default()
                },
            )?;
            let id = a["id"].as_str().unwrap().to_string();
            assert_eq!(a["sezione_id"], NON_ASSEGNATI);
            crate::sezioni::crea(c, "lavoro", "Lavoro", 1)?;
            let mut m = Map::new();
            m.insert("sezione_id".into(), Value::from("lavoro"));
            m.insert("fissato".into(), Value::Bool(true));
            let a = aggiorna(c, &id, &m)?;
            assert_eq!(a["sezione_id"], "lavoro");
            assert_eq!(a["fissato"], 1);
            assert_eq!(elenca(c, false)?.len(), 1);
            let a = archivia(c, &id)?;
            assert!(a["archiviato_ms"].is_i64());
            assert!(elenca(c, false)?.is_empty());
            assert!(crea(c, &NuovoAssistente::default()).is_err());
            assert_eq!(a["menzione"], "atena");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn menzioni_uniche() {
        let db = crate::db_prova();
        db.con(|c| {
            assert_eq!(menzione_base("Capo Squadra 140!"), "caposquadra140");
            assert_eq!(menzione_base("Ànna"), "ànna");
            for n in ["Atena", "ate na", "Atena2", "Altro"] {
                // Il più vecchio tiene la base: creati in millisecondi diversi (a parità di
                // `creato_ms` decide l'id, che è casuale).
                std::thread::sleep(std::time::Duration::from_millis(2));
                crea(
                    c,
                    &NuovoAssistente {
                        nome: n.into(),
                        ..Default::default()
                    },
                )?;
            }
            let m: Vec<String> = menzioni(c)?.into_iter().map(|(_, m)| m).collect();
            assert_eq!(m, ["atena", "atena3", "atena2", "altro"]);
            assert!(trova_per_menzione(c, "@ATENA3")?.is_some());
            assert!(trova_per_menzione(c, "nessuno")?.is_none());
            Ok(())
        })
        .unwrap();
    }
}
