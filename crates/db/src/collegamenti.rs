//! Collegamenti (server MCP per gli assistenti, Fase 2.D S2). Intestazioni e ambiente sono
//! cifrati con la chiave della cassaforte e non escono mai dalle rotte; gli strumenti visti
//! all'ultima prova stanno nell'impostazione riservata `server.collegamento.<id>.strumenti`.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use xbot_core::{Codice, Errore, Id, Risposta, SegretoTesto, nuovo_id, ora_ms};

use crate::cassaforte::Chiave;
use crate::impostazioni;
use crate::util::{RisultatoDb, in_transazione, tutte_json};

const PERMESSI: [&str; 3] = ["sempre", "chiedi", "bloccato"];

/// 2.L: il collegamento `c` vale per l'assistente `?1` se una riga di `collegamento_assistente`
/// non lo esclude (la riga vuol dire «escluso»).
pub(crate) const NON_ESCLUSO: &str = "NOT EXISTS (SELECT 1 FROM collegamento_assistente ca
    WHERE ca.collegamento_id = c.id AND ca.assistente_id = ?1 AND ca.abilitato = 1)";

/// Un collegamento nuovo (corpo di `POST /collegamenti`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NuovoCollegamento {
    pub nome: String,
    pub trasporto: String,
    pub comando: Option<String>,
    #[serde(default)]
    pub argomenti: Vec<String>,
    pub url: Option<String>,
    pub intestazioni: Option<Map<String, Value>>,
    pub ambiente: Option<Map<String, Value>>,
    pub categoria: Option<String>,
    pub istruzioni: Option<String>,
    /// 2.I: vuole un accesso OAuth (dal catalogo); il server lo mette anche da sé per gli url noti.
    #[serde(default)]
    pub richiede_oauth: Option<bool>,
    /// 2.K: il computer dove gira (stdio); senza, gira sul server.
    #[serde(default)]
    pub macchina_id: Option<String>,
}

/// Uno strumento visto con `tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StrumentoNoto {
    pub nome: String,
    pub descrizione: Option<String>,
    #[serde(default)]
    pub sola_lettura: bool,
}

/// Un collegamento pronto per un turno o per una prova, con i segreti decifrati.
#[derive(Debug, Clone)]
pub struct CollegamentoVivo {
    pub id: Id,
    pub nome: String,
    pub trasporto: String,
    pub comando: Option<String>,
    pub argomenti: Vec<String>,
    pub url: Option<String>,
    pub intestazioni: Vec<(String, SegretoTesto)>,
    pub ambiente: Vec<(String, SegretoTesto)>,
    /// 2.I: vuole l'accesso OAuth (catalogo, scoperta o riga `oauth_account`).
    pub richiede_oauth: bool,
    /// Lo stato dell'accesso OAuth (`ok`, `scaduto`, `revocato`), `None` se non c'è.
    pub oauth: Option<String>,
    /// 2.K: il computer dove gira; `None` = il server.
    pub macchina_id: Option<String>,
}

impl CollegamentoVivo {
    /// Entra nei turni: niente OAuth richiesto, oppure un accesso `ok` con il token iniettato.
    pub fn pronto(&self) -> bool {
        !self.richiede_oauth || self.oauth.as_deref() == Some("ok")
    }
}

fn chiave_strumenti(id: &str) -> String {
    format!("server.collegamento.{id}.strumenti")
}

fn controlla(
    trasporto: &str,
    comando: Option<&str>,
    url: Option<&str>,
    nome: &str,
) -> Risposta<()> {
    if nome.trim().is_empty() || nome.chars().count() > 80 {
        return Err(Errore::non_valida("il nome vuole da 1 a 80 caratteri"));
    }
    crate::controlli::testo_valido("nome", Some(nome), true)?;
    match trasporto {
        "stdio" if comando.is_some_and(|c| !c.trim().is_empty()) => Ok(()),
        "stdio" => Err(Errore::non_valida("un collegamento stdio vuole il comando")),
        "http" if url.is_some_and(|u| u.starts_with("https://") || u.starts_with("http://")) => {
            Ok(())
        }
        "http" => Err(Errore::non_valida(
            "un collegamento http vuole un url http(s)",
        )),
        _ => Err(Errore::non_valida("trasporto: stdio o http")),
    }
}

pub fn crea(conn: &Connection, chiave: Option<&Chiave>, n: &NuovoCollegamento) -> Risposta<Id> {
    controlla(
        &n.trasporto,
        n.comando.as_deref(),
        n.url.as_deref(),
        &n.nome,
    )?;
    crate::controlli::riferimento(conn, "macchina_id", n.macchina_id.as_deref())?;
    let id = nuovo_id();
    let vuota = Map::new();
    let intest = cifra_mappa(
        chiave,
        &id,
        "intestazioni",
        n.intestazioni.as_ref().unwrap_or(&vuota),
    )?;
    let amb = cifra_mappa(
        chiave,
        &id,
        "ambiente",
        n.ambiente.as_ref().unwrap_or(&vuota),
    )?;
    conn.execute(
        "INSERT INTO collegamento (id, nome, trasporto, comando, argomenti_json, url,
           intestazioni_cifrate, ambiente_cifrato, categoria, istruzioni, creato_ms,
           richiede_oauth, intestazioni_nomi_json, ambiente_nomi_json, macchina_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            id,
            n.nome.trim(),
            n.trasporto,
            n.comando,
            json!(n.argomenti).to_string(),
            n.url,
            intest,
            amb,
            n.categoria,
            n.istruzioni,
            ora_ms(),
            i64::from(n.richiede_oauth.unwrap_or(false)),
            nomi_json(n.intestazioni.as_ref().unwrap_or(&vuota)),
            nomi_json(n.ambiente.as_ref().unwrap_or(&vuota)),
            n.macchina_id.as_deref().filter(|m| !m.is_empty()),
        ],
    )
    .db()?;
    Ok(id)
}

fn esiste(conn: &Connection, id: &str) -> Risposta<()> {
    let c: Option<i64> = conn
        .query_row("SELECT 1 FROM collegamento WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()
        .db()?;
    c.map(|_| ())
        .ok_or_else(|| Errore::nuovo(Codice::NonTrovato, "collegamento non trovato"))
}

/// `PATCH /collegamenti/{id}`: campi parziali. `intestazioni` e `ambiente` (2.I) si fondono con
/// quelle salvate: una chiave a `null` si cancella, le altre si sostituiscono, le non nominate
/// restano; la mappa intera a `null` le cancella tutte.
pub fn aggiorna(
    conn: &Connection,
    chiave: Option<&Chiave>,
    id: &str,
    m: &Map<String, Value>,
) -> Risposta<()> {
    esiste(conn, id)?;
    const AMMESSI: [&str; 11] = [
        "macchina_id",
        "nome",
        "comando",
        "argomenti",
        "url",
        "categoria",
        "istruzioni",
        "abilitato",
        "intestazioni",
        "ambiente",
        "trasporto",
    ];
    if let Some(k) = m.keys().find(|k| !AMMESSI.contains(&k.as_str())) {
        return Err(Errore::non_valida(format!("campo non modificabile: {k}")));
    }
    in_transazione(conn, || {
        for (k, v) in m {
            let testo = |v: &Value| v.as_str().map(str::to_string);
            match k.as_str() {
                "argomenti" => {
                    let a: Vec<String> = serde_json::from_value(v.clone())
                        .map_err(|_| Errore::non_valida("argomenti: un elenco di testi"))?;
                    conn.execute(
                        "UPDATE collegamento SET argomenti_json = ?2 WHERE id = ?1",
                        params![id, json!(a).to_string()],
                    )
                    .db()?;
                }
                "abilitato" => {
                    let b = v
                        .as_bool()
                        .ok_or_else(|| Errore::non_valida("abilitato: vero o falso"))?;
                    conn.execute(
                        "UPDATE collegamento SET abilitato = ?2 WHERE id = ?1",
                        params![id, i64::from(b)],
                    )
                    .db()?;
                }
                "intestazioni" | "ambiente" => {
                    let (col, col_nomi) = if k == "ambiente" {
                        ("ambiente_cifrato", "ambiente_nomi_json")
                    } else {
                        ("intestazioni_cifrate", "intestazioni_nomi_json")
                    };
                    let mm = fondi(conn, chiave, id, k, col, v)?;
                    let blob = cifra_mappa(chiave, id, k, &mm)?;
                    conn.execute(
                        &format!(
                            "UPDATE collegamento SET {col} = ?2, {col_nomi} = ?3 WHERE id = ?1"
                        ),
                        params![id, blob, nomi_json(&mm)],
                    )
                    .db()?;
                }
                col => {
                    let mut t = testo(v);
                    if col == "macchina_id" {
                        t = t.filter(|m| !m.is_empty());
                        crate::controlli::riferimento(conn, col, t.as_deref())?;
                    }
                    conn.execute(
                        &format!("UPDATE collegamento SET {col} = ?2 WHERE id = ?1"),
                        params![id, t],
                    )
                    .db()?;
                }
            }
        }
        let (t, c, u, n): (String, Option<String>, Option<String>, String) = conn
            .query_row(
                "SELECT trasporto, comando, url, nome FROM collegamento WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .db()?;
        controlla(&t, c.as_deref(), u.as_deref(), &n)
    })
}

pub fn elimina(conn: &Connection, id: &str) -> Risposta<()> {
    esiste(conn, id)?;
    conn.execute("DELETE FROM collegamento WHERE id = ?1", [id])
        .db()?;
    impostazioni::scrivi(conn, &chiave_strumenti(id), &Value::Null)
}

/// Gli strumenti visti all'ultima prova.
pub fn strumenti(conn: &Connection, id: &str) -> Risposta<Vec<StrumentoNoto>> {
    Ok(impostazioni::leggi(conn, &chiave_strumenti(id))?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

/// L'esito di una prova: strumenti e `ultimo_ok_ms`, oppure `ultimo_errore`.
pub fn segna_prova(
    conn: &Connection,
    id: &str,
    esito: Result<&[StrumentoNoto], &str>,
) -> Risposta<()> {
    match esito {
        Ok(s) => {
            impostazioni::scrivi(conn, &chiave_strumenti(id), &json!(s))?;
            conn.execute(
                "UPDATE collegamento SET ultimo_ok_ms = ?2, ultimo_errore = NULL WHERE id = ?1",
                params![id, ora_ms()],
            )
            .db()?;
        }
        Err(e) => {
            conn.execute(
                "UPDATE collegamento SET ultimo_errore = ?2 WHERE id = ?1",
                params![id, e],
            )
            .db()?;
        }
    }
    Ok(())
}

/// Il permesso di uno strumento: quello suo, altrimenti `*lettura`/`*scrittura`, altrimenti `chiedi`.
pub fn permesso_per(conn: &Connection, id: &str, strumento: &str) -> Risposta<String> {
    let leggi = |s: &str| -> Risposta<Option<String>> {
        conn.query_row(
            "SELECT permesso FROM permesso_strumento WHERE collegamento_id = ?1 AND strumento = ?2",
            params![id, s],
            |r| r.get(0),
        )
        .optional()
        .db()
    };
    if let Some(p) = leggi(strumento)? {
        return Ok(p);
    }
    let lettura = strumenti(conn, id)?
        .iter()
        .any(|s| s.nome == strumento && s.sola_lettura);
    let generico = if lettura { "*lettura" } else { "*scrittura" };
    Ok(leggi(generico)?.unwrap_or_else(|| "chiedi".into()))
}

/// `GET /collegamenti/{id}` e l'elenco: mai intestazioni né ambiente, solo se ci sono.
pub fn leggi(conn: &Connection, id: &str) -> Risposta<Value> {
    esiste(conn, id)?;
    let mut v = tutte_json(
        conn,
        "SELECT id, nome, trasporto, comando, argomenti_json, url,
           intestazioni_cifrate IS NOT NULL AS intestazioni_presenti,
           ambiente_cifrato IS NOT NULL AS ambiente_presente, categoria, istruzioni,
           abilitato, creato_ms, ultimo_ok_ms, ultimo_errore, richiede_oauth,
           intestazioni_nomi_json, ambiente_nomi_json, macchina_id FROM collegamento WHERE id = ?1",
        [id],
    )?
    .pop()
    .unwrap_or(Value::Null);
    let esclusi: Vec<String> = {
        let mut st = conn
            .prepare("SELECT assistente_id FROM collegamento_assistente WHERE collegamento_id = ?1 AND abilitato = 1 ORDER BY assistente_id")
            .db()?;
        st.query_map([id], |r| r.get(0))
            .db()?
            .collect::<rusqlite::Result<_>>()
            .db()?
    };
    let mut strum = Vec::new();
    for s in strumenti(conn, id)? {
        let p = permesso_per(conn, id, &s.nome)?;
        strum.push(json!({ "nome": s.nome, "descrizione": s.descrizione, "sola_lettura": s.sola_lettura, "permesso": p }));
    }
    let permessi = tutte_json(
        conn,
        "SELECT strumento, permesso FROM permesso_strumento WHERE collegamento_id = ?1",
        [id],
    )?;
    let mut mp = Map::new();
    for p in permessi {
        if let (Some(s), Some(v)) = (p["strumento"].as_str(), p["permesso"].as_str()) {
            mp.insert(s.into(), v.into());
        }
    }
    if let Some(o) = v.as_object_mut() {
        for k in [
            "intestazioni_presenti",
            "ambiente_presente",
            "abilitato",
            "richiede_oauth",
        ] {
            let b = o.get(k).and_then(Value::as_i64).unwrap_or(0) == 1;
            o.insert(k.into(), Value::Bool(b));
        }
        let arg = o.remove("argomenti_json").unwrap_or(json!([]));
        o.insert("argomenti".into(), arg);
        o.insert("esclusi".into(), json!(esclusi));
        o.insert("strumenti".into(), Value::Array(strum));
        o.insert("permessi".into(), Value::Object(mp));
        // --- 2.I --- nomi (mai i valori) e accesso OAuth.
        for (da, a) in [
            ("intestazioni_nomi_json", "intestazioni_nomi"),
            ("ambiente_nomi_json", "ambiente_nomi"),
        ] {
            let n = o.remove(da).unwrap_or(json!([]));
            o.insert(a.into(), n);
        }
        let oauth = crate::oauth::pubblico(conn, id)?;
        if !oauth.is_null() {
            o.insert("richiede_oauth".into(), Value::Bool(true));
        }
        o.insert("oauth".into(), oauth);
        let client: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM oauth_account WHERE collegamento_id = ?1
                   AND client_id IS NOT NULL",
                [id],
                |r| r.get(0),
            )
            .db()?;
        o.insert("client_registrato".into(), Value::Bool(client));
    }
    Ok(v)
}

pub fn elenca(conn: &Connection) -> Risposta<Vec<Value>> {
    let ids: Vec<String> = {
        let mut st = conn
            .prepare("SELECT id FROM collegamento ORDER BY nome, id")
            .db()?;
        st.query_map([], |r| r.get(0))
            .db()?
            .collect::<rusqlite::Result<_>>()
            .db()?
    };
    ids.iter().map(|id| leggi(conn, id)).collect()
}

/// `PUT|POST /collegamenti/{id}/esclusi`: i bot per cui è spento (sostituisce l'elenco).
pub fn imposta_esclusi(conn: &Connection, id: &str, assistenti: &[String]) -> Risposta<()> {
    esiste(conn, id)?;
    in_transazione(conn, || {
        conn.execute(
            "DELETE FROM collegamento_assistente WHERE collegamento_id = ?1",
            [id],
        )
        .db()?;
        for a in assistenti {
            crate::controlli::riferimento(conn, "assistente", Some(a))?;
            conn.execute(
                "INSERT OR IGNORE INTO collegamento_assistente (collegamento_id, assistente_id) VALUES (?1, ?2)",
                params![id, a],
            )
            .db()?;
        }
        Ok(())
    })
}

/// `PUT /collegamenti/{id}/permessi`: `{"*lettura":"sempre", "<strumento>":"bloccato", …}`.
pub fn imposta_permessi(
    conn: &Connection,
    id: &str,
    permessi: &Map<String, Value>,
) -> Risposta<()> {
    esiste(conn, id)?;
    for (s, p) in permessi {
        if s.is_empty() || !p.as_str().is_some_and(|p| PERMESSI.contains(&p)) {
            return Err(Errore::non_valida(format!(
                "permesso di «{s}»: sempre, chiedi o bloccato"
            )));
        }
    }
    in_transazione(conn, || {
        conn.execute(
            "DELETE FROM permesso_strumento WHERE collegamento_id = ?1",
            [id],
        )
        .db()?;
        for (s, p) in permessi {
            conn.execute(
                "INSERT INTO permesso_strumento (collegamento_id, strumento, permesso) VALUES (?1, ?2, ?3)",
                params![id, s, p.as_str()],
            )
            .db()?;
        }
        Ok(())
    })
}

/// Quanti collegamenti sono accesi (`abilitato = 1`).
pub fn accesi(conn: &Connection) -> Risposta<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM collegamento WHERE abilitato = 1",
        [],
        |r| r.get(0),
    )
    .db()
}

mod segreti;
use segreti::decifra_mappa;
use segreti::{cifra_mappa, fondi, nomi_json};
mod vivi;
pub use vivi::{per_assistente, per_prova};
mod dove;
pub use dove::{fuori_dal_turno, per_assistente_su};

#[cfg(test)]
mod test;
