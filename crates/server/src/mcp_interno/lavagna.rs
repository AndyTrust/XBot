//! Gli strumenti della lavagna comune (`lavagna_leggi`, `lavagna_scrivi`, `lavagna_prendo_file`).
//! La logica sta in `xbot_db::lavagna`; qui si leggono gli argomenti e si dà il testo all'assistente.

use serde_json::{Map, Value, json};
use xbot_core::{GenereRiga, Ruolo};
use xbot_db::lavagna::{self, NuovaVoce};
use xbot_db::{assistenti, righe};

use super::TurnoMcp;
use super::specchio;
use super::strumenti::Esito;
use crate::Stato;
use crate::bus::Segnale;

fn testo<'a>(a: &'a Map<String, Value>, k: &str) -> &'a str {
    a.get(k).and_then(Value::as_str).unwrap_or_default().trim()
}

fn lista(a: &Map<String, Value>, k: &str) -> Vec<String> {
    a.get(k)
        .and_then(Value::as_array)
        .map(|v| {
            v.iter()
                .filter_map(|x| x.as_str())
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Una voce com'è mostrata all'assistente: niente campi interni.
fn vista(v: &Value) -> Value {
    json!({
        "agente": v["agente"], "progetto": v["progetto"], "cosa": v["cosa"],
        "file": v["file_json"], "risorse": v["risorse_json"],
        "dalle_ms": v["inizio_ms"], "origine": v["origine"],
    })
}

pub async fn leggi(m: &Stato, _t: &TurnoMcp, a: &Map<String, Value>) -> Esito {
    let progetto = Some(testo(a, "progetto"))
        .filter(|p| !p.is_empty())
        .map(str::to_string);
    let v =
        m.db.esegui(move |c| lavagna::attive(c, None, progetto.as_deref()))
            .await
            .map_err(|e| e.testo)?;
    Ok(json!({ "voci": v.iter().map(vista).collect::<Vec<_>>() }).to_string())
}

async fn agente(m: &Stato, assistente_id: &str) -> String {
    let aid = assistente_id.to_string();
    m.db.esegui(move |c| assistenti::leggi(c, &aid))
        .await
        .ok()
        .and_then(|a| a["menzione"].as_str().map(str::to_string))
        .unwrap_or_else(|| assistente_id.to_string())
}

pub async fn scrivi(m: &Stato, t: &TurnoMcp, a: &Map<String, Value>) -> Esito {
    let (aid, sid) = (t.assistente_id.clone(), t.sessione_id.clone());
    if a.get("finito").and_then(Value::as_bool).unwrap_or(false) {
        let esito = testo(a, "esito").to_string();
        let n =
            m.db.esegui(move |c| lavagna::chiudi(c, &aid, &sid, &esito))
                .await
                .map_err(|e| e.testo)?;
        specchio::subito(&m.db).await;
        return Ok(if n > 0 {
            "Voce chiusa.".into()
        } else {
            "Non avevi voci aperte.".into()
        });
    }
    let ag = agente(m, &t.assistente_id).await;
    let (progetto, cosa) = (
        testo(a, "progetto").to_string(),
        testo(a, "cosa").to_string(),
    );
    let (file, risorse) = (lista(a, "file"), lista(a, "risorse"));
    let voce =
        m.db.esegui(move |c| {
            lavagna::scrivi(
                c,
                &NuovaVoce {
                    assistente_id: &aid,
                    sessione_id: &sid,
                    agente: &ag,
                    macchina_id: None,
                    progetto: &progetto,
                    cosa: &cosa,
                    file: file.clone(),
                    risorse: risorse.clone(),
                },
            )
        })
        .await
        .map_err(|e| e.testo)?;
    specchio::subito(&m.db).await;
    avvisa_conflitti(m, t, &voce).await
}

pub async fn prendo_file(m: &Stato, t: &TurnoMcp, a: &Map<String, Value>) -> Esito {
    let nuovi = lista(a, "file");
    if nuovi.is_empty() {
        return Err("indica almeno un file o una cartella".into());
    }
    let ag = agente(m, &t.assistente_id).await;
    let (aid, sid) = (t.assistente_id.clone(), t.sessione_id.clone());
    let (progetto, cosa) = (
        testo(a, "progetto").to_string(),
        testo(a, "cosa").to_string(),
    );
    let risorse_nuove = lista(a, "risorse");
    let esito =
        m.db.esegui(move |c| {
            // I file si sommano a quelli che hai già; progetto e cosa restano se non li ridai.
            let prima = lavagna::voce_propria(c, &aid, &sid)?;
            let mut file: Vec<String> = prima
                .as_ref()
                .and_then(|v| v["file_json"].as_array())
                .map(|v| {
                    v.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let mut risorse: Vec<String> = prima
                .as_ref()
                .and_then(|v| v["risorse_json"].as_array())
                .map(|v| {
                    v.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            for f in &nuovi {
                if !file.contains(f) {
                    file.push(f.clone());
                }
            }
            for r in &risorse_nuove {
                if !risorse.contains(r) {
                    risorse.push(r.clone());
                }
            }
            let vecchio = |k: &str| {
                prima
                    .as_ref()
                    .and_then(|v| v[k].as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            let progetto = if progetto.is_empty() {
                vecchio("progetto")
            } else {
                progetto
            };
            let cosa = if !cosa.is_empty() {
                cosa
            } else if !vecchio("cosa").is_empty() {
                vecchio("cosa")
            } else {
                "lavoro sui file prenotati".to_string()
            };
            let (voce, conflitti) = lavagna::prendo_file(
                c,
                &NuovaVoce {
                    assistente_id: &aid,
                    sessione_id: &sid,
                    agente: &ag,
                    macchina_id: None,
                    progetto: &progetto,
                    cosa: &cosa,
                    file: file.clone(),
                    risorse: risorse.clone(),
                },
            )?;
            Ok((voce, conflitti))
        })
        .await
        .map_err(|e| e.testo)?;
    let (voce, conflitti) = esito;
    segnala(m, t, &conflitti).await;
    specchio::subito(&m.db).await;
    Ok(risposta(&voce, &conflitti))
}

async fn avvisa_conflitti(m: &Stato, t: &TurnoMcp, voce: &Value) -> Esito {
    let file: Vec<String> = voce["file_json"]
        .as_array()
        .map(|v| {
            v.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let risorse: Vec<String> = voce["risorse_json"]
        .as_array()
        .map(|v| {
            v.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let sid = t.sessione_id.clone();
    let conflitti =
        m.db.esegui(move |c| lavagna::conflitti_per(c, &sid, &file, &risorse))
            .await
            .map_err(|e| e.testo)?;
    segnala(m, t, &conflitti).await;
    Ok(risposta(voce, &conflitti))
}

fn risposta(voce: &Value, conflitti: &[Value]) -> String {
    let c: Vec<Value> = conflitti
        .iter()
        .map(|k| {
            json!({
                "occupato_da": k["agente"], "cosa": k["cosa"], "dalle_ms": k["inizio_ms"],
                "file_in_comune": k["in_conflitto"], "risorse_in_comune": k["risorse_in_conflitto"],
            })
        })
        .collect();
    let mut r = json!({ "prenotato": vista(voce), "conflitti": c });
    if !conflitti.is_empty() {
        r["avviso"] = json!(
            "Alcuni file o risorse sono già presi da un altro. Non scrivere sugli stessi file: \
             aspetta, scrivi al collega con @nome o avvisa l'utente."
        );
    }
    r.to_string()
}

/// Una riga `avviso` in chat per ogni conflitto, perché l'utente lo veda.
async fn segnala(m: &Stato, t: &TurnoMcp, conflitti: &[Value]) {
    if conflitti.is_empty() {
        return;
    }
    let chi: Vec<String> = conflitti
        .iter()
        .map(|k| k["agente"].as_str().unwrap_or("?").to_string())
        .collect();
    let corpo = json!({
        "codice": "lavagna_conflitto",
        "testo": format!("Lavagna: stessi file di {}", chi.join(", ")),
    });
    let sid = t.sessione_id.clone();
    let r = m
        .db
        .esegui(move |c| righe::aggiungi(c, &sid, GenereRiga::Avviso, Some(Ruolo::Sistema), &corpo))
        .await;
    match r {
        Ok(r) => m.motore.bus().pubblica(
            &t.sessione_id,
            Segnale::Righe {
                aggiornata_seq: r.aggiornata_seq,
            },
        ),
        Err(e) => tracing::warn!(errore = %e, "avviso della lavagna non scritto"),
    }
}
