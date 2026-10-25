//! Le voci della cassaforte come le usa l'API (contratto 2.D, sezione 1): crea, modifica
//! parziale, elimina, cerca, rivela, usi, segreti per assistente.

use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use xbot_core::{Codice, Errore, GenereCassaforte, Id, Risposta, SegretoTesto, ora_ms};
use zeroize::Zeroize;

use crate::cassaforte::{self, Chiave, NuovaVoce, VoceRiferimento};
use crate::cassaforte_generi::{normalizza, siti as pulisci_siti};
use crate::util::{RisultatoDb, in_transazione, tutte_json};

/// Il corpo di `POST` e `PATCH /cassaforte`. In PATCH tutto è facoltativo e `campi`
/// sostituisce solo le chiavi date (una chiave a `null` si toglie).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatiVoce {
    pub genere: Option<GenereCassaforte>,
    pub titolo: Option<String>,
    pub siti: Option<Vec<String>>,
    pub riferimento: Option<Map<String, Value>>,
    pub campi: Option<Map<String, Value>>,
    /// Solo per i segreti: gli assistenti che lo ricevono come variabile d'ambiente.
    pub assistenti: Option<Vec<String>>,
    #[serde(skip)]
    pub origine: Option<String>,
}

const MAX_TITOLO: usize = 200;

fn titolo_valido(t: &str) -> Risposta<String> {
    let t = t.trim();
    if t.is_empty() {
        return Err(Errore::non_valida("il titolo è obbligatorio"));
    }
    crate::controlli::testo_valido("titolo", Some(t), true)?;
    crate::controlli::lunghezza("titolo", Some(t), MAX_TITOLO)?;
    Ok(t.to_string())
}

fn serializza(m: &Map<String, Value>) -> SegretoTesto {
    SegretoTesto::from(Value::Object(m.clone()).to_string())
}

/// Crea una voce; restituisce l'id.
pub fn crea(conn: &Connection, chiave: &Chiave, d: &DatiVoce) -> Risposta<Id> {
    let genere = d
        .genere
        .ok_or_else(|| Errore::non_valida("il genere è obbligatorio"))?;
    let titolo = titolo_valido(d.titolo.as_deref().unwrap_or(""))?;
    let siti = pulisci_siti(d.siti.as_deref().unwrap_or(&[]))?;
    let (rif, campi) = normalizza(
        genere,
        &d.riferimento.clone().unwrap_or_default(),
        &d.campi.clone().unwrap_or_default(),
    )?;
    in_transazione(conn, || {
        let id = cassaforte::inserisci_voce(
            conn,
            chiave,
            &NuovaVoce {
                genere,
                titolo: titolo.clone(),
                siti: siti.clone(),
                riferimento: Value::Object(rif.clone()),
                campi: serializza(&campi),
                origine: Some(d.origine.clone().unwrap_or_else(|| "manuale".into())),
            },
        )?;
        if let Some(a) = &d.assistenti {
            imposta_assistenti(conn, &id, genere, a)?;
        }
        Ok(id)
    })
}

fn imposta_assistenti(
    conn: &Connection,
    id: &str,
    genere: GenereCassaforte,
    assistenti: &[String],
) -> Risposta<()> {
    if genere != GenereCassaforte::Segreto && !assistenti.is_empty() {
        return Err(Errore::non_valida(
            "assistenti: solo i segreti passano agli assistenti",
        ));
    }
    conn.execute("DELETE FROM segreto_assistente WHERE voce_id = ?1", [id])
        .db()?;
    for a in assistenti {
        crate::assistenti::leggi(conn, a)?;
        conn.execute(
            "INSERT OR IGNORE INTO segreto_assistente (voce_id, assistente_id) VALUES (?1, ?2)",
            params![id, a],
        )
        .db()?;
    }
    Ok(())
}

/// I campi in chiaro senza toccare l'ultimo uso.
fn campi_in_chiaro(conn: &Connection, chiave: &Chiave, id: &str) -> Risposta<Map<String, Value>> {
    let blob: Vec<u8> = conn
        .query_row(
            "SELECT campi_cifrati FROM cassaforte_voce WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()
        .db()?
        .ok_or_else(|| Errore::nuovo(Codice::VoceNonTrovata, "voce non trovata"))?;
    let mut chiaro = cassaforte::decifra(chiave, id, &blob)?;
    let v: Risposta<Value> = serde_json::from_slice(&chiaro)
        .map_err(|_| Errore::nuovo(Codice::Cifratura, "campi illeggibili"));
    chiaro.zeroize();
    match v? {
        Value::Object(m) => Ok(m),
        _ => Ok(Map::new()),
    }
}

fn unisci(base: &mut Map<String, Value>, nuovi: &Map<String, Value>) {
    for (k, v) in nuovi {
        if v.is_null() {
            base.remove(k);
        } else {
            base.insert(k.clone(), v.clone());
        }
    }
}

/// Modifica parziale. Il genere non cambia.
pub fn aggiorna(conn: &Connection, chiave: &Chiave, id: &str, d: &DatiVoce) -> Risposta<Value> {
    let attuale = cassaforte::leggi_riferimento(conn, id)?;
    let genere = GenereCassaforte::da_testo(&attuale.genere)?;
    if d.genere.is_some_and(|g| g != genere) {
        return Err(Errore::non_valida("il genere di una voce non cambia"));
    }
    in_transazione(conn, || {
        let ora = ora_ms();
        if let Some(t) = &d.titolo {
            conn.execute(
                "UPDATE cassaforte_voce SET titolo = ?2 WHERE id = ?1",
                params![id, titolo_valido(t)?],
            )
            .db()?;
        }
        if let Some(s) = &d.siti {
            let s = serde_json::to_string(&pulisci_siti(s)?)
                .map_err(|e| Errore::interno(e.to_string()))?;
            conn.execute(
                "UPDATE cassaforte_voce SET siti_json = ?2 WHERE id = ?1",
                params![id, s],
            )
            .db()?;
        }
        if d.riferimento.is_some() || d.campi.is_some() {
            let mut rif = attuale.riferimento.as_object().cloned().unwrap_or_default();
            unisci(&mut rif, &d.riferimento.clone().unwrap_or_default());
            let mut campi = campi_in_chiaro(conn, chiave, id)?;
            unisci(&mut campi, &d.campi.clone().unwrap_or_default());
            let (rif, campi) = normalizza(genere, &rif, &campi)?;
            let cifrati = cassaforte::cifra(chiave, id, serializza(&campi).esponi().as_bytes())?;
            conn.execute(
                "UPDATE cassaforte_voce SET riferimento_json = ?2, campi_cifrati = ?3 WHERE id = ?1",
                params![id, Value::Object(rif).to_string(), cifrati],
            )
            .db()?;
        }
        if let Some(a) = &d.assistenti {
            imposta_assistenti(conn, id, genere, a)?;
        }
        conn.execute(
            "UPDATE cassaforte_voce SET aggiornata_ms = ?2 WHERE id = ?1",
            params![id, ora],
        )
        .db()?;
        leggi(conn, id)
    })
}

pub fn elimina(conn: &Connection, id: &str) -> Risposta<()> {
    let n = conn
        .execute("DELETE FROM cassaforte_voce WHERE id = ?1", [id])
        .db()?;
    if n == 0 {
        return Err(Errore::nuovo(Codice::VoceNonTrovata, "voce non trovata"));
    }
    Ok(())
}

fn assistenti_di(conn: &Connection, id: &str) -> Risposta<Vec<String>> {
    let mut st = conn
        .prepare("SELECT assistente_id FROM segreto_assistente WHERE voce_id = ?1 ORDER BY 1")
        .db()?;
    st.query_map([id], |r| r.get(0))
        .db()?
        .collect::<rusqlite::Result<Vec<String>>>()
        .db()
}

fn vista(conn: &Connection, v: VoceRiferimento) -> Risposta<Value> {
    let assistenti = assistenti_di(conn, &v.id)?;
    Ok(json!({
        "id": v.id, "genere": v.genere, "titolo": v.titolo, "siti": v.siti,
        "riferimento": v.riferimento, "origine": v.origine, "creata_ms": v.creata_ms,
        "aggiornata_ms": v.aggiornata_ms, "ultimo_uso_ms": v.ultimo_uso_ms,
        "assistenti": assistenti,
    }))
}

/// Una voce come la vede l'elenco: mai i campi cifrati.
pub fn leggi(conn: &Connection, id: &str) -> Risposta<Value> {
    vista(conn, cassaforte::leggi_riferimento(conn, id)?)
}

/// Le voci di un genere (o tutte) che contengono `cerca` nel titolo, nei siti o nel riferimento.
pub fn cerca(
    conn: &Connection,
    genere: Option<GenereCassaforte>,
    cerca: Option<&str>,
) -> Risposta<Vec<Value>> {
    let ago = cerca
        .map(|c| c.trim().to_lowercase())
        .filter(|c| !c.is_empty());
    let mut fuori = Vec::new();
    for v in cassaforte::elenca(conn, genere)? {
        if let Some(a) = &ago {
            let pagliaio = format!("{} {} {}", v.titolo, v.siti, v.riferimento).to_lowercase();
            if !pagliaio.contains(a.as_str()) {
                continue;
            }
        }
        fuori.push(vista(conn, v)?);
    }
    Ok(fuori)
}

/// Registra un uso (`elenco`, `compila`, `riferimento`, `variabile_ambiente`).
pub fn registra_uso(
    conn: &Connection,
    voce_id: &str,
    sessione_id: Option<&str>,
    assistente_id: Option<&str>,
    azione: &str,
    richiesta_id: Option<&str>,
) -> Risposta<()> {
    conn.execute(
        "INSERT INTO cassaforte_uso (voce_id, sessione_id, assistente_id, azione, richiesta_id, ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            voce_id,
            sessione_id,
            assistente_id,
            azione,
            richiesta_id,
            ora_ms()
        ],
    )
    .db()?;
    Ok(())
}

/// I campi in chiaro, per l'utente dall'interfaccia: registra l'uso `riferimento`.
pub fn rivela(conn: &Connection, chiave: &Chiave, id: &str) -> Risposta<Value> {
    let testo = cassaforte::leggi_campi(conn, id, chiave)?;
    let campi: Value = serde_json::from_str(testo.esponi())
        .map_err(|_| Errore::nuovo(Codice::Cifratura, "campi illeggibili"))?;
    registra_uso(conn, id, None, None, "riferimento", None)?;
    Ok(json!({ "campi": campi }))
}

/// Gli usi di una voce, i più recenti prima.
pub fn usi(conn: &Connection, id: &str) -> Risposta<Vec<Value>> {
    cassaforte::leggi_riferimento(conn, id)?;
    tutte_json(
        conn,
        "SELECT u.ms, u.assistente_id, a.nome AS assistente, u.sessione_id, u.azione,
           u.richiesta_id
         FROM cassaforte_uso u LEFT JOIN assistente a ON a.id = u.assistente_id
         WHERE u.voce_id = ?1 ORDER BY u.ms DESC, u.id DESC LIMIT 200",
        [id],
    )
}

/// I segreti (genere `segreto`) concessi a un assistente, come (variabile, valore), da mettere
/// nell'ambiente del processo del provider. Registra l'uso `variabile_ambiente`.
pub fn segreti_per_assistente(
    conn: &Connection,
    chiave: &Chiave,
    assistente_id: &str,
    sessione_id: Option<&str>,
) -> Risposta<Vec<(String, SegretoTesto)>> {
    let mut st = conn
        .prepare(
            "SELECT v.id, json_extract(v.riferimento_json, '$.variabile')
             FROM segreto_assistente s JOIN cassaforte_voce v ON v.id = s.voce_id
             WHERE s.assistente_id = ?1 AND v.genere = 'segreto' ORDER BY v.titolo",
        )
        .db()?;
    let voci = st
        .query_map([assistente_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .db()?
        .collect::<rusqlite::Result<Vec<_>>>()
        .db()?;
    let mut fuori = Vec::new();
    for (id, variabile) in voci {
        let Some(variabile) = variabile else { continue };
        let campi = campi_in_chiaro(conn, chiave, &id)?;
        let Some(valore) = campi.get("valore").and_then(Value::as_str) else {
            continue;
        };
        fuori.push((variabile, SegretoTesto::nuovo(valore)));
        registra_uso(
            conn,
            &id,
            sessione_id,
            Some(assistente_id),
            "variabile_ambiente",
            None,
        )?;
        conn.execute(
            "UPDATE cassaforte_voce SET ultimo_uso_ms = ?2 WHERE id = ?1",
            params![id, ora_ms()],
        )
        .db()?;
    }
    Ok(fuori)
}

fn senza_accento(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ä' | 'ã' | 'å' => 'a',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ò' | 'ó' | 'ô' | 'ö' | 'õ' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        _ => c,
    }
}

/// Il nome della variabile d'ambiente per una voce chiesta con `cassaforte_chiedi`: la
/// `variabile` del riferimento (un segreto) se c'è, altrimenti `XBOT_CASSAFORTE_<TITOLO>` con il
/// titolo in maiuscolo, le lettere accentate senza accento e `_` al posto del resto.
pub fn nome_variabile(titolo: &str, riferimento: &Value) -> String {
    if let Some(v) = riferimento.get("variabile").and_then(Value::as_str) {
        let v = v.trim();
        if !v.is_empty() {
            return v.to_string();
        }
    }
    let mut s: String = titolo
        .to_lowercase()
        .chars()
        .map(senza_accento)
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    while s.contains("__") {
        s = s.replace("__", "_");
    }
    format!("XBOT_CASSAFORTE_{}", s.trim_matches('_'))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::assistenti::{NuovoAssistente, crea as crea_assistente};

    fn dati(v: Value) -> DatiVoce {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn giro_completo() {
        let db = crate::db_prova();
        let k = Chiave::da_byte([3u8; 32]);
        db.con(|c| {
            let id = crea(
                c,
                &k,
                &dati(
                    json!({"genere": "accesso", "titolo": "Posta", "siti": ["https://posta.it"],
                    "riferimento": {"utente": "mario"}, "campi": {"password": "uno", "note": "n"}}),
                ),
            )?;
            let v = aggiorna(
                c,
                &k,
                &id,
                &dati(json!({"campi": {"password": "due", "note": null}})),
            )?;
            assert_eq!(v["riferimento"]["utente"], "mario");
            // un campo cifrato già salvato si toglie con null; gli altri restano
            aggiorna(c, &k, &id, &dati(json!({"campi": {"pin": "1234"}})))?;
            assert_eq!(rivela(c, &k, &id)?["campi"]["pin"], "1234");
            aggiorna(c, &k, &id, &dati(json!({"campi": {"pin": null}})))?;
            assert_eq!(rivela(c, &k, &id)?["campi"], json!({"password": "due"}));
            let r = rivela(c, &k, &id)?;
            assert_eq!(r["campi"], json!({"password": "due"}));
            assert_eq!(usi(c, &id)?[0]["azione"], "riferimento");
            assert_eq!(cerca(c, None, Some("POSTA"))?.len(), 1);
            assert!(cerca(c, None, Some("nulla"))?.is_empty());
            assert!(aggiorna(c, &k, &id, &dati(json!({"genere": "carta"}))).is_err());
            elimina(c, &id)?;
            assert_eq!(elimina(c, &id).unwrap_err().codice, Codice::VoceNonTrovata);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn segreti_agli_assistenti() {
        let db = crate::db_prova();
        let k = Chiave::da_byte([4u8; 32]);
        db.con(|c| {
            let a = crea_assistente(
                c,
                &NuovoAssistente {
                    nome: "J".into(),
                    ..Default::default()
                },
            )?;
            let aid = a["id"].as_str().unwrap().to_string();
            crea(
                c,
                &k,
                &dati(json!({"genere": "segreto", "titolo": "GitHub",
                    "riferimento": {"variabile": "GH_TOKEN"}, "campi": {"valore": "ghp_x"},
                    "assistenti": [aid]})),
            )?;
            let s = segreti_per_assistente(c, &k, &aid, None)?;
            assert_eq!(s.len(), 1);
            assert_eq!((s[0].0.as_str(), s[0].1.esponi()), ("GH_TOKEN", "ghp_x"));
            let e = crea(
                c,
                &k,
                &dati(
                    json!({"genere": "accesso", "titolo": "x", "campi": {"password": "p"},
                    "assistenti": [aid]}),
                ),
            );
            assert!(e.is_err());
            assert_eq!(
                nome_variabile("Banca Intesa / web", &json!({})),
                "XBOT_CASSAFORTE_BANCA_INTESA_WEB"
            );
            assert_eq!(
                nome_variabile("Prova parità", &json!({})),
                "XBOT_CASSAFORTE_PROVA_PARITA"
            );
            assert_eq!(
                nome_variabile("Prova", &json!({"variabile": "PROVA_PARITA_KEY"})),
                "PROVA_PARITA_KEY"
            );
            Ok(())
        })
        .unwrap();
    }
}
