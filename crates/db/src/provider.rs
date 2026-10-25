//! Provider, modelli e quote (l'anello per provider).

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use xbot_core::{Codice, Errore, Ms, Risposta, ora_ms};

use crate::util::{RisultatoDb, tutte_json};

/// Registra (o aggiorna) un provider caricato dal motore. Non tocca `attivo`.
pub fn registra(conn: &Connection, id: &str, nome: &str, programma: &str) -> Risposta<()> {
    conn.execute(
        "INSERT INTO provider (id, nome, programma, creato_ms) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET nome = excluded.nome, programma = excluded.programma,
           macchina_id = NULL",
        params![id, nome, programma, ora_ms()],
    )
    .db()?;
    Ok(())
}

/// Un modello come lo dichiara il provider.
#[derive(Debug, Clone)]
pub struct DatiModello<'a> {
    pub id: &'a str,
    pub etichetta: &'a str,
    pub dettaglio: Option<&'a str>,
    pub livelli: &'a [String],
    pub ordine: i64,
}

pub fn registra_modello(conn: &Connection, provider_id: &str, m: &DatiModello<'_>) -> Risposta<()> {
    let livelli = serde_json::to_string(m.livelli)
        .map_err(|e| Errore::interno(format!("livelli non serializzabili: {e}")))?;
    conn.execute(
        "INSERT INTO modello (id, provider_id, etichetta, dettaglio, livelli_ragionamento_json, ordine)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET provider_id = excluded.provider_id,
           etichetta = excluded.etichetta, dettaglio = excluded.dettaglio,
           livelli_ragionamento_json = excluded.livelli_ragionamento_json, ordine = excluded.ordine",
        params![m.id, provider_id, m.etichetta, m.dettaglio, livelli, m.ordine],
    )
    .db()?;
    Ok(())
}

pub fn elenca(conn: &Connection) -> Risposta<Vec<Value>> {
    tutte_json(
        conn,
        "SELECT id, nome, programma, argomenti_json, macchina_id, attivo, creato_ms
         FROM provider ORDER BY creato_ms, id",
        [],
    )
}

/// I modelli visibili, di un provider o di tutti.
pub fn modelli(conn: &Connection, provider_id: Option<&str>) -> Risposta<Vec<Value>> {
    tutte_json(
        conn,
        "SELECT id, provider_id, etichetta, dettaglio, livelli_ragionamento_json, disponibile, ordine
         FROM modello WHERE nascosto = 0 AND (?1 IS NULL OR provider_id = ?1)
         ORDER BY provider_id, ordine, id",
        [provider_id],
    )
}

/// Vero se il provider esiste ed è attivo.
pub fn attivo(conn: &Connection, id: &str) -> Risposta<bool> {
    let a: Option<i64> = conn
        .query_row("SELECT attivo FROM provider WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()
        .db()?;
    Ok(a == Some(1))
}

/// Il primo provider attivo fra quelli dati, nell'ordine dato («auto»).
pub fn primo_attivo(conn: &Connection, candidati: &[String]) -> Risposta<Option<String>> {
    for c in candidati {
        if attivo(conn, c)? {
            return Ok(Some(c.clone()));
        }
    }
    Ok(None)
}

/// Errore `modello_non_disponibile` se il modello non è di quel provider o non è disponibile.
/// Mai ripiego silenzioso su un altro modello.
pub fn verifica_modello(conn: &Connection, provider_id: &str, modello_id: &str) -> Risposta<()> {
    let ok: Option<i64> = conn
        .query_row(
            "SELECT disponibile FROM modello WHERE id = ?1 AND provider_id = ?2",
            params![modello_id, provider_id],
            |r| r.get(0),
        )
        .optional()
        .db()?;
    if ok == Some(1) {
        Ok(())
    } else {
        Err(Errore::nuovo(
            Codice::ModelloNonDisponibile,
            format!("il modello {modello_id} non è disponibile su {provider_id}"),
        ))
    }
}

/// Registra una rilevazione della quota. La percentuale si tiene fra 0 e 100.
pub fn rileva_quota(
    conn: &Connection,
    provider_id: &str,
    percentuale_usata: f64,
    azzera_ms: Option<Ms>,
    fonte: &str,
    dettaglio: Option<&Value>,
) -> Risposta<()> {
    let percentuale = if percentuale_usata.is_finite() {
        percentuale_usata.clamp(0.0, 100.0)
    } else {
        0.0
    };
    conn.execute(
        "INSERT INTO quota_rilevazione
           (provider_id, rilevata_ms, percentuale_usata, azzera_ms, fonte, dettaglio_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            provider_id,
            ora_ms(),
            percentuale,
            azzera_ms,
            fonte,
            dettaglio.map(Value::to_string)
        ],
    )
    .db()?;
    Ok(())
}

/// I provider attivi, nell'ordine di «auto»: (id, programma).
pub fn attivi(conn: &Connection) -> Risposta<Vec<(String, String)>> {
    let mut st = conn
        .prepare(
            "SELECT id, programma FROM provider WHERE attivo = 1 AND macchina_id IS NULL
                  ORDER BY creato_ms, id",
        )
        .db()?;
    let v = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .db()?
        .collect::<rusqlite::Result<Vec<_>>>()
        .db()?;
    Ok(v)
}

/// Accende o spegne un provider.
pub fn imposta_attivo(conn: &Connection, id: &str, attivo: bool) -> Risposta<()> {
    conn.execute(
        "UPDATE provider SET attivo = ?2 WHERE id = ?1",
        params![id, i64::from(attivo)],
    )
    .db()?;
    Ok(())
}

/// L'anello per provider (`v_quota_corrente`) con il dettaglio dell'ultima rilevazione
/// (per Claude Code: le finestre `five_hour` e `seven_day`) più i turni degli ultimi 7 giorni.
/// `costo_7g` somma `turno.costo_usd` (dollari, come li dichiara il client).
pub fn quote(conn: &Connection) -> Risposta<Vec<Value>> {
    let da = ora_ms() - 7 * 24 * 3600 * 1000;
    tutte_json(
        conn,
        "SELECT q.*,
           (SELECT dettaglio_json FROM quota_rilevazione r WHERE r.provider_id = q.provider_id
             ORDER BY r.rilevata_ms DESC, r.id DESC LIMIT 1) AS dettaglio_json,
           (SELECT COUNT(*) FROM turno t WHERE t.provider_id = q.provider_id AND t.iniziato_ms >= ?1) AS turni_7g,
           (SELECT COALESCE(SUM(token_ingresso), 0) FROM turno t WHERE t.provider_id = q.provider_id AND t.iniziato_ms >= ?1) AS token_ingresso_7g,
           (SELECT COALESCE(SUM(token_uscita), 0) FROM turno t WHERE t.provider_id = q.provider_id AND t.iniziato_ms >= ?1) AS token_uscita_7g,
           (SELECT COALESCE(SUM(costo_usd), 0.0) FROM turno t WHERE t.provider_id = q.provider_id AND t.iniziato_ms >= ?1) AS costo_7g
         FROM v_quota_corrente q ORDER BY q.provider_id",
        [da],
    )
}

// --- S-B (2.E) --- provider dichiarati dai computer collegati.

/// Un modello come lo dichiara il lavoratore.
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct ModelloDichiarato {
    pub id: String,
    #[serde(default)]
    pub etichetta: String,
    pub dettaglio: Option<String>,
    #[serde(default)]
    pub livelli: Vec<String>,
}

/// Un provider come lo dichiara il lavoratore (`provider[]` di `collega` e di `battito`).
#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize)]
pub struct ProviderDichiarato {
    pub id: String,
    #[serde(default)]
    pub nome: String,
    #[serde(default)]
    pub programma: String,
    pub versione: Option<String>,
    #[serde(default)]
    pub modelli: Vec<ModelloDichiarato>,
    /// 2.I: l'account del client su quella macchina, `{nome, dettaglio}` (`None` = non collegato).
    #[serde(default)]
    pub account: Option<Value>,
}

fn id_valido(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':'))
}

/// Registra un provider su una macchina: la riga `provider` se manca (con `macchina_id` =
/// questa macchina: non è un provider del server), `provider_macchina` e i modelli. Un modello
/// con lo stesso id di un altro provider si salva come `<provider>/<id>`.
pub fn registra_su_macchina(
    conn: &Connection,
    macchina_id: &str,
    p: &ProviderDichiarato,
) -> Risposta<()> {
    if !id_valido(&p.id) || p.id.contains('/') {
        return Err(Errore::non_valida(format!(
            "id di provider non valido: {}",
            p.id
        )));
    }
    let ora = ora_ms();
    let nome = if p.nome.trim().is_empty() {
        &p.id
    } else {
        &p.nome
    };
    conn.execute(
        "INSERT INTO provider (id, nome, programma, macchina_id, creato_ms) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO NOTHING",
        params![p.id, nome, p.programma, macchina_id, ora],
    )
    .db()?;
    conn.execute(
        "INSERT INTO provider_macchina (provider_id, macchina_id, programma, versione, visto_ms, account_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(provider_id, macchina_id) DO UPDATE SET programma = excluded.programma,
           versione = excluded.versione, visto_ms = excluded.visto_ms, account_json = excluded.account_json",
        params![p.id, macchina_id, p.programma, p.versione, ora, p.account.as_ref().map(Value::to_string)],
    )
    .db()?;
    // Un provider che sta solo sui computer (non sul server): i suoi modelli sono quelli
    // dichiarati adesso. Quelli spariti si nascondono (disponibile = 0, nascosto = 1) invece di
    // cancellarli, così sessioni e assistenti che li puntano restano e il turno dice
    // `modello_non_disponibile`.
    let solo_computer: bool = conn
        .query_row(
            "SELECT macchina_id IS NOT NULL FROM provider WHERE id = ?1",
            [&p.id],
            |r| r.get(0),
        )
        .db()?;
    let mut dichiarati: Vec<String> = Vec::new();
    for (i, m) in p.modelli.iter().enumerate() {
        if !id_valido(&m.id) {
            continue;
        }
        let altrui: Option<String> = conn
            .query_row(
                "SELECT provider_id FROM modello WHERE id = ?1",
                [&m.id],
                |r| r.get(0),
            )
            .optional()
            .db()?;
        let id = match altrui {
            Some(pr) if pr != p.id => format!("{}/{}", p.id, m.id),
            _ => m.id.clone(),
        };
        let etichetta = if m.etichetta.trim().is_empty() {
            &m.id
        } else {
            &m.etichetta
        };
        registra_modello(
            conn,
            &p.id,
            &DatiModello {
                id: &id,
                etichetta,
                dettaglio: m.dettaglio.as_deref(),
                livelli: &m.livelli,
                ordine: i64::try_from(i).unwrap_or(0),
            },
        )?;
        dichiarati.push(id);
    }
    if solo_computer {
        let lista = serde_json::to_string(&dichiarati)
            .map_err(|e| Errore::interno(format!("modelli: {e}")))?;
        conn.execute(
            "UPDATE modello SET
               disponibile = CASE WHEN id IN (SELECT value FROM json_each(?2)) THEN 1 ELSE 0 END,
               nascosto = CASE WHEN id IN (SELECT value FROM json_each(?2)) THEN 0 ELSE 1 END
             WHERE provider_id = ?1",
            params![p.id, lista],
        )
        .db()?;
    }
    Ok(())
}

/// Le macchine (non rimosse) dove gira un provider, dalla più recente.
pub fn macchine_del_provider(conn: &Connection, provider_id: &str) -> Risposta<Vec<String>> {
    let mut st = conn
        .prepare(
            "SELECT pm.macchina_id FROM provider_macchina pm JOIN macchina m ON m.id = pm.macchina_id
             WHERE pm.provider_id = ?1 AND m.rimossa_ms IS NULL ORDER BY pm.visto_ms DESC",
        )
        .db()?;
    let v = st
        .query_map([provider_id], |r| r.get(0))
        .db()?
        .collect::<rusqlite::Result<Vec<_>>>()
        .db()?;
    Ok(v)
}

/// I provider di una macchina, nell'ordine di «auto» (prima `claude_code`).
pub fn della_macchina(conn: &Connection, macchina_id: &str) -> Risposta<Vec<String>> {
    let mut st = conn
        .prepare(
            "SELECT pm.provider_id FROM provider_macchina pm JOIN provider p ON p.id = pm.provider_id
             WHERE pm.macchina_id = ?1 AND p.attivo = 1
             ORDER BY pm.provider_id != 'claude_code', p.creato_ms, pm.provider_id",
        )
        .db()?;
    let v = st
        .query_map([macchina_id], |r| r.get(0))
        .db()?
        .collect::<rusqlite::Result<Vec<_>>>()
        .db()?;
    Ok(v)
}

/// Per ogni provider le macchine dove gira: `{id, nome, genere, stato, account, versione}`. Il
/// server si aggiunge per i provider caricati dal suo registro (`locali`), con l'account letto da
/// `imposta_account_server`.
pub fn macchine_per_provider(
    conn: &Connection,
    server_id: &str,
    locali: &[String],
) -> Risposta<std::collections::HashMap<String, Vec<Value>>> {
    let mut mappa: std::collections::HashMap<String, Vec<Value>> = Default::default();
    let server = tutte_json(
        conn,
        "SELECT id, nome, genere, stato FROM macchina WHERE id = ?1",
        [server_id],
    )?;
    for l in locali {
        let letto = crate::impostazioni::leggi(conn, &chiave_account(l))?.unwrap_or(Value::Null);
        mappa
            .entry(l.clone())
            .or_default()
            .extend(server.iter().map(|m| {
                let mut m = m.clone();
                m["account"] = letto["account"].clone();
                m["versione"] = letto["versione"].clone();
                m
            }));
    }
    for r in tutte_json(
        conn,
        "SELECT pm.provider_id, m.id, m.nome, m.genere, m.stato, pm.versione, pm.account_json
         FROM provider_macchina pm
         JOIN macchina m ON m.id = pm.macchina_id WHERE m.rimossa_ms IS NULL
         ORDER BY m.registrata_ms",
        [],
    )? {
        let p = r["provider_id"].as_str().unwrap_or("").to_string();
        mappa.entry(p).or_default().push(serde_json::json!({
            "id": r["id"], "nome": r["nome"], "genere": r["genere"], "stato": r["stato"],
            "versione": r["versione"], "account": r["account_json"],
        }));
    }
    Ok(mappa)
}
// --- fine S-B ---

// --- 2.I --- account dei client del server e fonte della quota.
fn chiave_account(provider_id: &str) -> String {
    format!("server.provider.{provider_id}.account")
}

/// L'account (e la versione) di un client del server, letto ogni 5 minuti.
pub fn imposta_account_server(
    conn: &Connection,
    provider_id: &str,
    account: Option<&Value>,
    versione: Option<&str>,
) -> Risposta<()> {
    let v = serde_json::json!({ "account": account, "versione": versione, "letto_ms": ora_ms() });
    crate::impostazioni::scrivi(conn, &chiave_account(provider_id), &v)
}

/// `client` se il client ha dichiarato la quota negli ultimi 7 giorni, altrimenti `stimata`.
pub fn fonte_quota(conn: &Connection, provider_id: &str) -> Risposta<&'static str> {
    let da = ora_ms() - 7 * 24 * 3600 * 1000;
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM quota_rilevazione WHERE provider_id = ?1 AND fonte = 'client'
               AND rilevata_ms >= ?2",
            params![provider_id, da],
            |r| r.get(0),
        )
        .db()?;
    Ok(if n > 0 { "client" } else { "stimata" })
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn registra_modelli_e_quote() {
        let db = crate::db_prova();
        db.con(|c| {
            registra(c, "finto", "Finto", "interno")?;
            registra(c, "finto", "Finto 2", "interno")?;
            registra_modello(
                c,
                "finto",
                &DatiModello {
                    id: "finto-1",
                    etichetta: "Finto 1",
                    dettaglio: None,
                    livelli: &["basso".into()],
                    ordine: 0,
                },
            )?;
            assert_eq!(elenca(c)?.len(), 1);
            assert_eq!(elenca(c)?[0]["nome"], "Finto 2");
            assert_eq!(
                modelli(c, Some("finto"))?[0]["livelli_ragionamento_json"][0],
                "basso"
            );
            assert!(verifica_modello(c, "finto", "finto-1").is_ok());
            assert_eq!(
                verifica_modello(c, "finto", "altro").unwrap_err().codice,
                Codice::ModelloNonDisponibile
            );
            assert_eq!(
                primo_attivo(c, &["nessuno".into(), "finto".into()])?.as_deref(),
                Some("finto")
            );
            rileva_quota(c, "finto", 27.0, None, "stima", None)?;
            let q = quote(c)?;
            assert_eq!(q[0]["percentuale_usata"], 27.0);
            std::thread::sleep(std::time::Duration::from_millis(3));
            let d = serde_json::json!({ "five_hour": { "percentuale_usata": 12.0 } });
            rileva_quota(c, "finto", 240.0, Some(5), "client", Some(&d))?;
            let q = quote(c)?;
            assert_eq!(q[0]["percentuale_usata"], 100.0);
            assert_eq!(
                q[0]["dettaglio_json"]["five_hour"]["percentuale_usata"],
                12.0
            );
            assert_eq!(
                attivi(c)?,
                vec![("finto".to_string(), "interno".to_string())]
            );
            imposta_attivo(c, "finto", false)?;
            assert!(attivi(c)?.is_empty());
            assert_eq!(q[0]["turni_7g"], 0);
            Ok(())
        })
        .unwrap();
    }
}
