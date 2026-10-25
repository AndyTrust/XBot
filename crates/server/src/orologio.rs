//! L'orologio delle routine: a ogni scadenza crea un invio nella sessione corrente
//! dell'assistente (`origine = routine:<id>`) e registra la `routine_esecuzione`.
//!
//! Attivatori: `cron` `{"expression": "30 7 * * *"}` (5 campi, ora locale del server; anche 6-7
//! campi con i secondi), `ogni_secondi` `{"every_seconds": 3600}` (minimo 5), `evento`
//! `{"evento": "posta_nuova"}` (non scade mai da solo: parte con «esegui» o da chi genera
//! l'evento).

use std::str::FromStr;
use std::time::Duration;

use chrono::{Local, TimeZone};
use serde_json::{Value, json};
use xbot_core::{Errore, Ms, Risposta, nuovo_id, ora_ms};
use xbot_db::{routine, sessioni};

use crate::Stato;
use crate::motore::DatiInvio;

pub const MIN_SECONDI: i64 = 5;

fn espressione(e: &str) -> Risposta<cron::Schedule> {
    let campi = e.split_whitespace().count();
    let completa = match campi {
        5 => format!("0 {e}"),
        6 | 7 => e.to_string(),
        _ => {
            return Err(Errore::non_valida(
                "cron: servono 5 campi (min ora giorno mese settimana)",
            ));
        }
    };
    cron::Schedule::from_str(&completa)
        .map_err(|e| Errore::non_valida(format!("cron non valido: {e}")))
}

/// Controlla l'attivatore e lo restituisce normalizzato.
pub fn controlla(tipo: &str, j: &Value) -> Risposta<Value> {
    match tipo {
        "cron" => {
            let e = j["expression"]
                .as_str()
                .or(j["espressione"].as_str())
                .ok_or_else(|| Errore::non_valida("cron: manca expression"))?;
            espressione(e)?;
            Ok(json!({ "expression": e.trim() }))
        }
        "ogni_secondi" => {
            let n = j["every_seconds"]
                .as_i64()
                .or(j["secondi"].as_i64())
                .ok_or_else(|| Errore::non_valida("ogni_secondi: manca every_seconds"))?;
            if n < MIN_SECONDI {
                return Err(Errore::non_valida(format!(
                    "ogni_secondi: almeno {MIN_SECONDI}"
                )));
            }
            Ok(json!({ "every_seconds": n }))
        }
        "evento" => {
            let e = j["evento"]
                .as_str()
                .filter(|e| !e.trim().is_empty())
                .ok_or_else(|| Errore::non_valida("evento: manca il nome dell'evento"))?;
            Ok(json!({ "evento": e.trim() }))
        }
        _ => Err(Errore::non_valida(
            "attivatore_tipo vuole cron, ogni_secondi o evento",
        )),
    }
}

/// La prossima scadenza dopo `dopo` (ms); `None` per gli eventi.
pub fn prossima(tipo: &str, j: &Value, dopo: Ms) -> Risposta<Option<Ms>> {
    match tipo {
        "cron" => {
            let s = espressione(j["expression"].as_str().unwrap_or(""))?;
            let da = Local
                .timestamp_millis_opt(dopo)
                .single()
                .ok_or_else(|| Errore::interno("ora non rappresentabile"))?;
            Ok(s.after(&da).next().map(|t| t.timestamp_millis()))
        }
        "ogni_secondi" => Ok(Some(
            dopo + j["every_seconds"].as_i64().unwrap_or(3600) * 1000,
        )),
        _ => Ok(None),
    }
}

/// La sessione dove scrive la routine: la corrente dell'assistente se è attiva, altrimenti
/// la più recente attiva, altrimenti una nuova con il nome della routine.
fn sessione_per(c: &xbot_db::Connessione, assistente: &str, nome: &str) -> Risposta<String> {
    let corrente: Option<String> = c
        .query_row(
            "SELECT s.id FROM stato_lettura sl JOIN sessione s ON s.id = sl.sessione_corrente_id
             WHERE sl.assistente_id = ?1 AND s.stato = 'attiva'",
            [assistente],
            |r| r.get(0),
        )
        .ok();
    if let Some(id) = corrente {
        return Ok(id);
    }
    if let Some(s) = sessioni::elenca(c, assistente, false)?
        .into_iter()
        .find(|s| s.stato == "attiva")
    {
        return Ok(s.id);
    }
    let n = sessioni::NuovaSessione {
        titolo: Some(nome.chars().take(200).collect()),
        ..Default::default()
    };
    Ok(sessioni::crea(c, assistente, &n)?.id)
}

/// Esegue una routine adesso: crea l'invio e registra l'esecuzione. `sposta` = calcola la
/// prossima scadenza (per «esegui» a mano la scadenza resta com'è).
pub async fn esegui(s: &Stato, r: &Value, sposta: bool) -> Risposta<Value> {
    let id = r["id"].as_str().unwrap_or("").to_string();
    let assistente = r["assistente_id"].as_str().unwrap_or("").to_string();
    let nome = r["nome"].as_str().unwrap_or("Routine").to_string();
    let tipo = r["attivatore_tipo"].as_str().unwrap_or("").to_string();
    let att = r["attivatore_json"].clone();
    let prossima_ms = if sposta {
        prossima(&tipo, &att, ora_ms()).unwrap_or(None)
    } else {
        r["prossima_ms"].as_i64()
    };
    let (a2, n2) = (assistente.clone(), nome.clone());
    // --- S-B (2.E) --- la routine gira sulla macchina predefinita dell'assistente (se è un
    // computer spento vale la regola del motore: «auto» ripiega qui, un client fisso aspetta).
    let a3 = assistente.clone();
    let macchina =
        s.db.esegui(move |c| {
            Ok(
                xbot_db::assistenti::leggi(c, &a3)?["macchina_predefinita_id"]
                    .as_str()
                    .map(str::to_string),
            )
        })
        .await
        .ok()
        .flatten();
    // --- fine S-B ---
    let sessione = s.db.esegui(move |c| sessione_per(c, &a2, &n2)).await;
    let (sid, esito) = match sessione {
        Ok(sid) => {
            let d = DatiInvio {
                id: nuovo_id(),
                testo: r["corpo"].as_str().unwrap_or("").to_string(),
                allegati: json!([]),
                risposta_a: None,
                provider: None,
                macchina,
                origine: format!("routine:{id}"),
                composto_ms: None,
            };
            match s.motore.invia(&sid, d).await {
                Ok(e) => (
                    Some(sid),
                    json!({"invio_id": e.invio_id, "stato": e.stato, "turno_id": e.turno_id}),
                ),
                Err(e) => (Some(sid), json!({"stato": "fallito", "codice": e.codice})),
            }
        }
        Err(e) => (None, json!({"stato": "fallito", "codice": e.codice})),
    };
    let testo_esito = esito["stato"].as_str().unwrap_or("fallito").to_string();
    let (id2, sid2) = (id.clone(), sid.clone());
    let esecuzione =
        s.db.esegui(move |c| {
            routine::registra_esecuzione(c, &id2, sid2.as_deref(), &testo_esito, prossima_ms)
        })
        .await?;
    tracing::info!(routine = %id, esito = %esito["stato"], "routine eseguita");
    Ok(
        json!({ "esecuzione_id": esecuzione, "sessione_id": sid, "invio": esito, "prossima_ms": prossima_ms }),
    )
}

/// Un giro: esegue le routine scadute. Restituisce quante.
pub async fn giro(s: &Stato) -> usize {
    let scadute = match s.db.esegui(|c| routine::scadute(c, ora_ms())).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(errore = %e, "orologio: lettura fallita");
            return 0;
        }
    };
    let mut n = 0;
    for r in scadute {
        match esegui(s, &r, true).await {
            Ok(_) => n += 1,
            Err(e) => tracing::warn!(errore = %e, "routine non eseguita"),
        }
    }
    n
}

pub fn avvia(s: Stato) {
    tokio::spawn(async move {
        let mut t = tokio::time::interval(Duration::from_secs(1));
        t.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            t.tick().await;
            giro(&s).await;
        }
    });
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn attivatori_e_scadenze() {
        assert_eq!(
            controlla("cron", &json!({"expression": "30 7 * * *"})).unwrap()["expression"],
            "30 7 * * *"
        );
        assert!(controlla("cron", &json!({"expression": "ogni tanto"})).is_err());
        assert!(controlla("cron", &json!({"expression": "61 7 * * *"})).is_err());
        assert!(controlla("ogni_secondi", &json!({"every_seconds": 4})).is_err());
        assert_eq!(
            controlla("ogni_secondi", &json!({"secondi": 5})).unwrap()["every_seconds"],
            5
        );
        assert!(controlla("evento", &json!({})).is_err());
        assert!(controlla("webhook", &json!({})).is_err());
        let ora = ora_ms();
        assert_eq!(
            prossima("ogni_secondi", &json!({"every_seconds": 5}), ora).unwrap(),
            Some(ora + 5000)
        );
        let p = prossima("cron", &json!({"expression": "*/1 * * * *"}), ora)
            .unwrap()
            .unwrap();
        assert!(p > ora && p <= ora + 60_000);
        let giorno = prossima("cron", &json!({"expression": "0 3 * * *"}), ora)
            .unwrap()
            .unwrap();
        assert!(giorno > ora && giorno <= ora + 86_400_000 + 3_600_000);
        assert_eq!(
            prossima("evento", &json!({"evento": "x"}), ora).unwrap(),
            None
        );
    }
}
