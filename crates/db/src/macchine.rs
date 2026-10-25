//! Macchine collegate: il server stesso (registrato all'avvio, con battito), i biglietti per
//! collegarne altre (Fase 2.E), il permesso di esecuzione, gli ultimi lavori e turni.

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use xbot_core::{Codice, Errore, PermessoEsecuzione, Risposta, ora_ms};

use crate::util::{RisultatoDb, tutte_json, una_json};

const COLONNE: &str =
    "id, nome, etichetta, genere, os, arch, versione_lavoratore, posti, stato, permesso_esecuzione,
    capacita_json, ultimo_battito_ms, registrata_ms";

fn con_radici(conn: &Connection, mut m: Value) -> Risposta<Value> {
    let id = m["id"].as_str().unwrap_or("").to_string();
    // --- S-B (2.E) --- provider della macchina e capacità.
    m["provider"] = Value::Array(tutte_json(
        conn,
        "SELECT pm.provider_id AS id, p.nome, pm.programma, pm.versione, pm.visto_ms
         FROM provider_macchina pm JOIN provider p ON p.id = pm.provider_id
         WHERE pm.macchina_id = ?1 ORDER BY pm.provider_id",
        [&id],
    )?);
    if let Some(ps) = m["provider"].as_array_mut() {
        for p in ps {
            let pid = p["id"].as_str().unwrap_or("").to_string();
            p["modelli"] = Value::Array(crate::provider::modelli(conn, Some(&pid))?);
        }
    }
    m["capacita"] = m["capacita_json"].clone();
    // --- fine S-B ---
    m["radici"] = Value::Array(tutte_json(
        conn,
        "SELECT id, etichetta, percorso, permessi_json FROM radice WHERE macchina_id = ?1 ORDER BY etichetta",
        [&id],
    )?);
    Ok(m)
}

pub fn elenca(conn: &Connection) -> Risposta<Vec<Value>> {
    tutte_json(
        conn,
        &format!("SELECT {COLONNE} FROM macchina WHERE rimossa_ms IS NULL ORDER BY registrata_ms"),
        [],
    )?
    .into_iter()
    .map(|m| con_radici(conn, m))
    .collect()
}

pub fn leggi(conn: &Connection, id: &str) -> Risposta<Value> {
    let m = una_json(
        conn,
        &format!("SELECT {COLONNE} FROM macchina WHERE id = ?1"),
        [id],
    )?
    .ok_or_else(|| Errore::nuovo(Codice::MacchinaNonTrovata, "macchina non trovata"))?;
    con_radici(conn, m)
}

/// Il server registra sé stesso (o si aggiorna) come macchina `connessa`.
pub fn registra_se_stessa(
    conn: &Connection,
    id: &str,
    nome: &str,
    genere: &str,
    os: &str,
    arch: &str,
) -> Risposta<()> {
    let ora = ora_ms();
    conn.execute(
        "INSERT INTO macchina (id, nome, genere, chiave_pubblica, os, arch, versione_lavoratore,
           stato, capacita_json, ultimo_battito_ms, registrata_ms)
         VALUES (?1, ?2, ?3, '', ?4, ?5, ?6, 'connessa', '[\"turni\"]', ?7, ?7)
         ON CONFLICT(id) DO UPDATE SET nome = excluded.nome, os = excluded.os, arch = excluded.arch,
           versione_lavoratore = excluded.versione_lavoratore, stato = 'connessa',
           ultimo_battito_ms = excluded.ultimo_battito_ms, rimossa_ms = NULL",
        params![id, nome, genere, os, arch, xbot_core::VERSIONE, ora],
    )
    .db()?;
    Ok(())
}

/// Il battito: la macchina è viva adesso.
pub fn battito(conn: &Connection, id: &str) -> Risposta<()> {
    conn.execute(
        "UPDATE macchina SET ultimo_battito_ms = ?2, stato = 'connessa' WHERE id = ?1",
        params![id, ora_ms()],
    )
    .db()?;
    Ok(())
}

/// Alla chiusura del server.
pub fn ferma(conn: &Connection, id: &str) -> Risposta<()> {
    conn.execute("UPDATE macchina SET stato = 'ferma' WHERE id = ?1", [id])
        .db()?;
    Ok(())
}

pub fn imposta_permesso(conn: &Connection, id: &str, p: PermessoEsecuzione) -> Risposta<Value> {
    let n = conn
        .execute(
            "UPDATE macchina SET permesso_esecuzione = ?2 WHERE id = ?1",
            params![id, p.come_testo()],
        )
        .db()?;
    if n == 0 {
        return Err(Errore::nuovo(
            Codice::MacchinaNonTrovata,
            "macchina non trovata",
        ));
    }
    leggi(conn, id)
}

/// Gli ultimi lavori della macchina e gli ultimi turni che ci sono girati (per il server
/// anche quelli senza macchina, che girano sulla VPS).
pub fn lavori(conn: &Connection, id: &str, limite: i64, e_il_server: bool) -> Risposta<Value> {
    leggi(conn, id)?;
    let limite = limite.clamp(1, 200);
    let lavori = tutte_json(
        conn,
        "SELECT id, sessione_id, turno_id, capacita, stato, creato_ms, preso_ms, finito_ms,
           codice_uscita, CASE WHEN finito_ms IS NOT NULL THEN finito_ms - creato_ms END AS durata_ms
         FROM lavoro WHERE macchina_id = ?1 ORDER BY creato_ms DESC LIMIT ?2",
        params![id, limite],
    )?;
    let turni = tutte_json(
        conn,
        "SELECT t.id, t.sessione_id, s.assistente_id, a.nome AS assistente, t.provider_id,
           t.modello_id, t.stato, t.iniziato_ms, t.finito_ms, t.durata_ms, t.errore_codice
         FROM turno t JOIN sessione s ON s.id = t.sessione_id JOIN assistente a ON a.id = s.assistente_id
         WHERE t.macchina_id = ?1 OR (?3 AND t.macchina_id IS NULL)
         ORDER BY t.iniziato_ms DESC LIMIT ?2",
        params![id, limite, e_il_server],
    )?;
    Ok(json!({ "lavori": lavori, "turni": turni }))
}

/// Salva l'hash di un biglietto per collegare un computer.
pub fn crea_biglietto(conn: &Connection, hash: &str, scade_ms: i64) -> Risposta<()> {
    conn.execute(
        "INSERT INTO biglietto_macchina (hash, creato_ms, scade_ms) VALUES (?1, ?2, ?3)",
        params![hash, ora_ms(), scade_ms],
    )
    .db()?;
    Ok(())
}

/// Vero se il biglietto esiste, non è scaduto e non è stato usato (per la Fase 2.E).
pub fn biglietto_valido(conn: &Connection, hash: &str) -> Risposta<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM biglietto_macchina WHERE hash = ?1 AND usato_ms IS NULL AND scade_ms > ?2",
            params![hash, ora_ms()],
            |_| Ok(()),
        )
        .optional()
        .db()?
        .is_some())
}

/// «Continua su…»: la sessione passa a una macchina `connessa`. Scrive il riassunto di
/// passaggio (gli ultimi 20 messaggi, troncati), dimentica la sessione del client (che stava
/// sull'altra macchina) e aggiunge una riga `evento`. Restituisce (riassunto, aggiornata_seq).
pub fn sposta_sessione(
    conn: &Connection,
    sessione_id: &str,
    macchina_id: &str,
) -> Risposta<(String, i64)> {
    let s = crate::sessioni::leggi(conn, sessione_id)?;
    let m = leggi(conn, macchina_id)?;
    if m["stato"] != "connessa" {
        return Err(Errore::nuovo(
            Codice::MacchinaNonConnessa,
            "la macchina non è connessa",
        ));
    }
    let mut st = conn
        .prepare(
            "SELECT ruolo, json_extract(corpo_json, '$.testo') FROM riga
             WHERE sessione_id = ?1 AND genere = 'messaggio' AND soppressa = 0
             ORDER BY seq DESC LIMIT 20",
        )
        .db()?;
    let mut righe = st
        .query_map([sessione_id], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?,
            ))
        })
        .db()?
        .collect::<rusqlite::Result<Vec<_>>>()
        .db()?;
    righe.reverse();
    let nome_assistente = crate::assistenti::leggi(conn, &s.assistente_id)?["nome"]
        .as_str()
        .unwrap_or("Assistente")
        .to_string();
    let nome_utente = crate::util::nome_utente(conn);
    let riassunto = righe
        .into_iter()
        .filter_map(|(ruolo, testo)| {
            let t = testo?.split_whitespace().collect::<Vec<_>>().join(" ");
            let chi = match ruolo.as_deref() {
                Some("utente") => nome_utente.as_str(),
                Some("collega") => "Collega",
                _ => nome_assistente.as_str(),
            };
            let t = if t.chars().count() > 300 {
                format!("{}…", t.chars().take(299).collect::<String>())
            } else {
                t
            };
            Some(format!("{chi}: {t}"))
        })
        .collect::<Vec<_>>()
        .join("\n");
    crate::util::in_transazione(conn, || {
        conn.execute(
            "UPDATE sessione SET macchina_id = ?2, riassunto_passaggio = ?3, aggiornata_ms = ?4,
               sessione_esterna_id = CASE WHEN macchina_id IS ?2 THEN sessione_esterna_id END
             WHERE id = ?1",
            params![sessione_id, macchina_id, riassunto, ora_ms()],
        )
        .db()?;
        let riga = crate::righe::aggiungi(
            conn,
            sessione_id,
            xbot_core::GenereRiga::Evento,
            Some(xbot_core::Ruolo::Sistema),
            &json!({ "tipo": "spostamento", "macchina_id": macchina_id, "macchina": m["nome"],
                     "testo": format!("Sessione spostata su {}", m["nome"].as_str().unwrap_or(macchina_id)) }),
        )?;
        Ok((riassunto.clone(), riga.aggiornata_seq))
    })
}

// --- S-B (2.E) --- collegare, riconoscere e rimuovere un computer con il lavoratore.

/// Il corpo di `POST /macchine/collega` (senza il biglietto).
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct NuovaMacchina {
    pub nome: String,
    #[serde(default = "genere_mac")]
    pub genere: String,
    pub os: Option<String>,
    pub arch: Option<String>,
    pub versione_lavoratore: Option<String>,
    #[serde(default)]
    pub chiave_pubblica: String,
    pub posti: Option<i64>,
    #[serde(default)]
    pub capacita: Vec<String>,
    #[serde(default)]
    pub provider: Vec<crate::provider::ProviderDichiarato>,
}

fn genere_mac() -> String {
    "mac".into()
}

/// Usa il biglietto (una volta sola, se non è scaduto) e registra la macchina `connessa` con
/// l'hash della credenziale. Registra anche i provider dichiarati. Restituisce l'id.
pub fn collega(
    conn: &Connection,
    hash_biglietto: &str,
    n: &NuovaMacchina,
    hash_credenziale: &str,
) -> Risposta<String> {
    if n.nome.trim().is_empty() || n.nome.chars().count() > 80 {
        return Err(Errore::non_valida("nome: da 1 a 80 caratteri"));
    }
    if !["mac", "windows", "vps", "android", "ios"].contains(&n.genere.as_str()) {
        return Err(Errore::non_valida(
            "genere vuole mac, windows, vps, android o ios",
        ));
    }
    let posti = n.posti.unwrap_or(1).clamp(1, 4);
    let capacita = serde_json::to_string(&n.capacita)
        .map_err(|e| Errore::interno(format!("capacità: {e}")))?;
    crate::util::in_transazione(conn, || {
        let ora = ora_ms();
        let id = xbot_core::nuovo_id();
        let usato = conn
            .execute(
                "UPDATE biglietto_macchina SET usato_ms = ?2
                 WHERE hash = ?1 AND usato_ms IS NULL AND scade_ms > ?2",
                params![hash_biglietto, ora],
            )
            .db()?;
        if usato == 0 {
            return Err(Errore::nuovo(
                Codice::NonAutorizzato,
                "biglietto non valido, già usato o scaduto",
            ));
        }
        conn.execute(
            "INSERT INTO macchina (id, nome, genere, chiave_pubblica, os, arch, versione_lavoratore,
               posti, stato, capacita_json, ultimo_battito_ms, registrata_ms, credenziale_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'connessa', ?9, ?10, ?10, ?11)",
            params![
                id,
                n.nome.trim(),
                n.genere,
                n.chiave_pubblica,
                n.os,
                n.arch,
                n.versione_lavoratore,
                posti,
                capacita,
                ora,
                hash_credenziale
            ],
        )
        .db()?;
        conn.execute(
            "UPDATE biglietto_macchina SET macchina_id = ?2 WHERE hash = ?1",
            params![hash_biglietto, id],
        )
        .db()?;
        for p in &n.provider {
            crate::provider::registra_su_macchina(conn, &id, p)?;
        }
        Ok(id)
    })
}

/// La macchina (non rimossa) con questa credenziale: `(id, nome)`.
pub fn per_credenziale(conn: &Connection, hash: &str) -> Risposta<Option<(String, String)>> {
    conn.query_row(
        "SELECT id, nome FROM macchina WHERE credenziale_hash = ?1 AND rimossa_ms IS NULL",
        [hash],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
    .db()
}

/// Cambia lo stato (`connessa`, `riconnessione`, `ferma`) di una macchina non rimossa.
pub fn imposta_stato(conn: &Connection, id: &str, stato: &str) -> Risposta<()> {
    conn.execute(
        "UPDATE macchina SET stato = ?2 WHERE id = ?1 AND rimossa_ms IS NULL",
        params![id, stato],
    )
    .db()?;
    Ok(())
}

/// Il battito del lavoratore: macchina `connessa`, provider dichiarati aggiornati.
pub fn battito_lavoratore(
    conn: &Connection,
    id: &str,
    provider: &[crate::provider::ProviderDichiarato],
) -> Risposta<()> {
    crate::util::in_transazione(conn, || {
        battito(conn, id)?;
        for p in provider {
            crate::provider::registra_su_macchina(conn, id, p)?;
        }
        Ok(())
    })
}

/// Rimuove una macchina: credenziale revocata, provider tolti, lavori aperti annullati.
pub fn rimuovi(conn: &Connection, id: &str) -> Risposta<()> {
    leggi(conn, id)?;
    crate::util::in_transazione(conn, || {
        let ora = ora_ms();
        conn.execute(
            "UPDATE macchina SET stato = 'rimossa', rimossa_ms = ?2, credenziale_hash = NULL
             WHERE id = ?1",
            params![id, ora],
        )
        .db()?;
        conn.execute("DELETE FROM provider_macchina WHERE macchina_id = ?1", [id])
            .db()?;
        conn.execute(
            "UPDATE lavoro SET stato = 'annullato', finito_ms = ?2
             WHERE macchina_id = ?1 AND stato IN ('in_coda','preso','in_corso')",
            params![id, ora],
        )
        .db()?;
        conn.execute(
            "UPDATE assistente SET macchina_predefinita_id = NULL WHERE macchina_predefinita_id = ?1",
            [id],
        )
        .db()?;
        Ok(())
    })
}

/// Le macchine con lavoratore (credenziale) e il loro ultimo battito: `(id, stato, battito)`.
pub fn con_lavoratore(conn: &Connection) -> Risposta<Vec<(String, String, Option<i64>)>> {
    let mut st = conn
        .prepare(
            "SELECT id, stato, ultimo_battito_ms FROM macchina
             WHERE credenziale_hash IS NOT NULL AND rimossa_ms IS NULL",
        )
        .db()?;
    let v = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .db()?
        .collect::<rusqlite::Result<Vec<_>>>()
        .db()?;
    Ok(v)
}
// --- fine S-B ---
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn se_stessa_permesso_e_biglietti() {
        let db = crate::db_prova();
        db.con(|c| {
            assert!(elenca(c)?.is_empty());
            registra_se_stessa(c, "m1", "vps", "vps", "linux", "x86_64")?;
            registra_se_stessa(c, "m1", "vps", "vps", "linux", "x86_64")?;
            battito(c, "m1")?;
            let v = elenca(c)?;
            assert_eq!((v.len(), v[0]["stato"].as_str()), (1, Some("connessa")));
            assert!(v[0]["radici"].as_array().unwrap().is_empty());
            let m = imposta_permesso(c, "m1", PermessoEsecuzione::Never)?;
            assert_eq!(m["permesso_esecuzione"], "never");
            assert_eq!(
                imposta_permesso(c, "no", PermessoEsecuzione::Ask)
                    .unwrap_err()
                    .codice,
                Codice::MacchinaNonTrovata
            );
            assert!(
                lavori(c, "m1", 10, true)?["turni"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            crea_biglietto(c, "h", ora_ms() + 60_000)?;
            assert!(biglietto_valido(c, "h")? && !biglietto_valido(c, "x")?);
            ferma(c, "m1")?;
            Ok(())
        })
        .unwrap();
    }
}
