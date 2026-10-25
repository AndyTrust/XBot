//! Canali (Fase 2.D S2): client esterni (altri bot e agenti) che usano XBot via MCP.
//! Del token si salva solo l'hash SHA-256; il token si mostra una volta, alla creazione.

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use xbot_core::{Codice, Errore, Id, Risposta, ora_ms};

use crate::util::{RisultatoDb, tutte_json};

/// Gli strumenti che un canale può ricevere.
pub const STRUMENTI: [&str; 14] = [
    "leggi_memoria",
    "cerca_memoria",
    "scrivi_quaderno",
    "elenco_assistenti",
    "invia_a_assistente",
    "stato_quote",
    "cassaforte_elenco",
    "posta_elenca",
    "posta_leggi",
    "posta_cerca",
    "posta_bozza_o_invia",
    // 2026-10-25: n8n-xbot fa partire le routine di XBot e ne formatta l'esito.
    "esegui_routine",
    "esito_routine",
    // 2026-10-25: gli agenti di fuori scrivono lo stato nella memoria comune.
    "scrivi_memoria",
];

/// Un canale autenticato: quello che serve al server MCP.
#[derive(Debug, Clone)]
pub struct CanaleAttivo {
    pub id: Id,
    pub nome: String,
    pub assistente_id: Option<String>,
    pub strumenti: Vec<String>,
    /// Quanti `invia_a_assistente` in un'ora (0 = nessuno).
    pub limite_orario: i64,
}

fn controlla_strumenti(s: &[String]) -> Risposta<()> {
    match s.iter().find(|x| !STRUMENTI.contains(&x.as_str())) {
        Some(x) => Err(Errore::non_valida(format!("strumento sconosciuto: {x}"))),
        None => Ok(()),
    }
}

/// Crea il canale con l'hash del token già calcolato.
pub fn crea(
    conn: &Connection,
    id: &str,
    nome: &str,
    assistente_id: Option<&str>,
    strumenti: &[String],
    token_hash: &str,
) -> Risposta<()> {
    if nome.trim().is_empty() || nome.chars().count() > 80 {
        return Err(Errore::non_valida("il nome vuole da 1 a 80 caratteri"));
    }
    crate::controlli::testo_valido("nome", Some(nome), true)?;
    controlla_strumenti(strumenti)?;
    crate::controlli::riferimento(conn, "assistente", assistente_id)?;
    conn.execute(
        "INSERT INTO canale (id, nome, token_hash, assistente_id, strumenti_json, creato_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            id,
            nome.trim(),
            token_hash,
            assistente_id,
            json!(strumenti).to_string(),
            ora_ms()
        ],
    )
    .db()?;
    Ok(())
}

fn esiste(conn: &Connection, id: &str) -> Risposta<()> {
    let c: Option<i64> = conn
        .query_row("SELECT 1 FROM canale WHERE id = ?1", [id], |r| r.get(0))
        .optional()
        .db()?;
    c.map(|_| ())
        .ok_or_else(|| Errore::nuovo(Codice::NonTrovato, "canale non trovato"))
}

/// `GET /canali`: mai l'hash del token.
pub fn elenca(conn: &Connection) -> Risposta<Vec<Value>> {
    let inizio_giorno = ora_ms() - ora_ms().rem_euclid(86_400_000);
    let mut v = tutte_json(
        conn,
        "SELECT c.id, c.nome, c.assistente_id, c.strumenti_json, c.creato_ms, c.ultimo_uso_ms,
           c.revocato_ms, c.limite_orario,
           (SELECT COUNT(*) FROM canale_chiamata k WHERE k.canale_id = c.id AND k.ms >= ?1) AS chiamate_oggi
         FROM canale c ORDER BY c.creato_ms DESC",
        [inizio_giorno],
    )?;
    for c in &mut v {
        if let Some(o) = c.as_object_mut() {
            let s = o.remove("strumenti_json").unwrap_or(json!([]));
            o.insert("strumenti".into(), s);
            let revocato = o.get("revocato_ms").is_some_and(|r| !r.is_null());
            o.insert("revocato".into(), Value::Bool(revocato));
        }
    }
    Ok(v)
}

/// Revoca: il token non vale più (la riga resta per il registro). Dalla 2.I un canale già
/// revocato si elimina davvero, con il suo registro delle chiamate.
pub fn revoca(conn: &Connection, id: &str) -> Risposta<()> {
    esiste(conn, id)?;
    let gia: bool = conn
        .query_row(
            "SELECT revocato_ms IS NOT NULL FROM canale WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .db()?;
    if gia {
        conn.execute("DELETE FROM canale WHERE id = ?1", [id])
            .db()?;
        return Ok(());
    }
    conn.execute(
        "UPDATE canale SET revocato_ms = COALESCE(revocato_ms, ?2) WHERE id = ?1",
        params![id, ora_ms()],
    )
    .db()?;
    Ok(())
}

/// Un token nuovo per lo stesso canale; un canale revocato resta revocato (409).
pub fn rigenera(conn: &Connection, id: &str, token_hash: &str) -> Risposta<()> {
    esiste(conn, id)?;
    let revocato: bool = conn
        .query_row(
            "SELECT revocato_ms IS NOT NULL FROM canale WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .db()?;
    if revocato {
        return Err(Errore::nuovo(
            Codice::CanaleRevocato,
            "il canale è revocato: creane uno nuovo",
        ));
    }
    conn.execute(
        "UPDATE canale SET token_hash = ?2 WHERE id = ?1",
        params![id, token_hash],
    )
    .db()?;
    Ok(())
}

/// Il canale non revocato con questo hash; aggiorna `ultimo_uso_ms`.
pub fn per_hash(conn: &Connection, token_hash: &str) -> Risposta<Option<CanaleAttivo>> {
    let r: Option<(String, String, Option<String>, String, i64)> = conn
        .query_row(
            "SELECT id, nome, assistente_id, strumenti_json, limite_orario FROM canale
             WHERE token_hash = ?1 AND revocato_ms IS NULL",
            [token_hash],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()
        .db()?;
    let Some((id, nome, assistente_id, s, limite_orario)) = r else {
        return Ok(None);
    };
    conn.execute(
        "UPDATE canale SET ultimo_uso_ms = ?2 WHERE id = ?1",
        params![id, ora_ms()],
    )
    .db()?;
    Ok(Some(CanaleAttivo {
        id,
        nome,
        assistente_id,
        strumenti: serde_json::from_str(&s).unwrap_or_default(),
        limite_orario,
    }))
}

/// Gli invii del canale nell'ultima ora (invii con `origine = canale:<id>`).
pub fn invii_ultima_ora(conn: &Connection, id: &str) -> Risposta<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM invio WHERE origine = ?1 AND inviato_ms >= ?2",
        params![format!("canale:{id}"), ora_ms() - 3_600_000],
        |r| r.get(0),
    )
    .db()
}

/// `PATCH /canali/{id}`: `nome`, `assistente_id`, `strumenti`, `limite_orario`.
pub fn aggiorna(conn: &Connection, id: &str, m: &serde_json::Map<String, Value>) -> Risposta<()> {
    esiste(conn, id)?;
    if let Some(k) = m
        .keys()
        .find(|k| !["nome", "assistente_id", "strumenti", "limite_orario"].contains(&k.as_str()))
    {
        return Err(Errore::non_valida(format!("campo non modificabile: {k}")));
    }
    crate::util::in_transazione(conn, || {
        if let Some(v) = m.get("nome") {
            let n = v.as_str().unwrap_or_default();
            if n.trim().is_empty() || n.chars().count() > 80 {
                return Err(Errore::non_valida("il nome vuole da 1 a 80 caratteri"));
            }
            crate::controlli::testo_valido("nome", Some(n), true)?;
            conn.execute(
                "UPDATE canale SET nome = ?2 WHERE id = ?1",
                params![id, n.trim()],
            )
            .db()?;
        }
        if let Some(v) = m.get("assistente_id") {
            let a = v.as_str().filter(|a| !a.is_empty());
            crate::controlli::riferimento(conn, "assistente", a)?;
            conn.execute(
                "UPDATE canale SET assistente_id = ?2 WHERE id = ?1",
                params![id, a],
            )
            .db()?;
        }
        if let Some(v) = m.get("strumenti") {
            let s: Vec<String> = serde_json::from_value(v.clone())
                .map_err(|_| Errore::non_valida("strumenti: un elenco di nomi"))?;
            controlla_strumenti(&s)?;
            conn.execute(
                "UPDATE canale SET strumenti_json = ?2 WHERE id = ?1",
                params![id, json!(s).to_string()],
            )
            .db()?;
        }
        if let Some(v) = m.get("limite_orario") {
            let n = v
                .as_i64()
                .filter(|n| (0..=1000).contains(n))
                .ok_or_else(|| Errore::non_valida("limite_orario: un intero da 0 a 1000"))?;
            conn.execute(
                "UPDATE canale SET limite_orario = ?2 WHERE id = ?1",
                params![id, n],
            )
            .db()?;
        }
        Ok(())
    })
}

/// Una chiamata nel registro. `argomenti` arriva già privato dei segreti.
pub fn registra_chiamata(
    conn: &Connection,
    canale_id: &str,
    strumento: &str,
    argomenti: &Value,
    esito: &str,
    durata_ms: i64,
) -> Risposta<()> {
    conn.execute(
        "INSERT INTO canale_chiamata (canale_id, strumento, argomenti_json, esito, ms, durata_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            canale_id,
            strumento,
            argomenti.to_string(),
            esito,
            ora_ms(),
            durata_ms
        ],
    )
    .db()?;
    Ok(())
}

/// `GET /canali/{id}/chiamate?limite=`: le più recenti prima.
pub fn chiamate(conn: &Connection, id: &str, limite: i64) -> Risposta<Vec<Value>> {
    esiste(conn, id)?;
    tutte_json(
        conn,
        "SELECT ms, strumento, esito, durata_ms, argomenti_json FROM canale_chiamata
         WHERE canale_id = ?1 ORDER BY ms DESC, id DESC LIMIT ?2",
        params![id, limite.clamp(1, 500)],
    )
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn giro_completo() {
        let db = crate::db_prova();
        db.con(|c| {
            let s = vec!["elenco_assistenti".to_string()];
            crea(c, "k1", "grok-bot", None, &s, "h1")?;
            assert!(crea(c, "k2", "x", None, &["inventato".into()], "h2").is_err());
            assert!(crea(c, "k3", "x", Some("nessuno"), &s, "h3").is_err());
            let a = per_hash(c, "h1")?.unwrap();
            assert_eq!(a.strumenti, s);
            registra_chiamata(c, "k1", "elenco_assistenti", &json!({}), "ok", 3)?;
            let v = elenca(c)?;
            assert_eq!(v[0]["chiamate_oggi"], 1);
            assert_eq!(v[0]["revocato"], false);
            assert!(!v[0].to_string().contains("h1"));
            assert_eq!(chiamate(c, "k1", 10)?[0]["esito"], "ok");
            rigenera(c, "k1", "h9")?;
            assert!(per_hash(c, "h1")?.is_none());
            assert_eq!(per_hash(c, "h9")?.unwrap().limite_orario, 30);
            let mut m = serde_json::Map::new();
            m.insert("limite_orario".into(), json!(2));
            aggiorna(c, "k1", &m)?;
            assert_eq!(per_hash(c, "h9")?.unwrap().limite_orario, 2);
            m.insert("limite_orario".into(), json!(-1));
            assert!(aggiorna(c, "k1", &m).is_err());
            assert_eq!(invii_ultima_ora(c, "k1")?, 0);
            revoca(c, "k1")?;
            assert!(per_hash(c, "h9")?.is_none());
            assert_eq!(
                rigenera(c, "k1", "h10").unwrap_err().codice,
                Codice::CanaleRevocato
            );
            assert!(revoca(c, "zz").is_err());
            Ok(())
        })
        .unwrap();
    }
}
