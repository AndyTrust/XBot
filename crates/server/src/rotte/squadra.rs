//! `GET /squadra` (Fase 2.E S-B): chi è ogni assistente, cosa fa, con cosa lavora, dove, le sue
//! routine attive e l'ultimo turno. Gli stessi dati entrano nel testo di sistema (`sistema.rs`).

use axum::extract::State;
use serde_json::{Value, json};
use xbot_core::Risposta;
use xbot_db::{Connessione, assistenti, tutte_json, una_json};

use super::json_db;
use crate::errore_http::RisHttp;
use crate::stato::Stato;

/// La squadra (assistenti non archiviati). `server_id` è la macchina predefinita di chi non ne
/// ha una; `connesse` dice quali computer hanno il canale aperto.
pub fn squadra(c: &Connessione, server_id: &str, connesse: &[String]) -> Risposta<Vec<Value>> {
    let mut v = Vec::new();
    for a in assistenti::elenca(c, false)? {
        let id = a["id"].as_str().unwrap_or("").to_string();
        let provider = match a["provider_predefinito_id"].as_str() {
            Some(p) => una_json(c, "SELECT id, nome FROM provider WHERE id = ?1", [p])?
                .unwrap_or_else(|| json!({ "id": p, "nome": p })),
            None => json!({ "id": "auto", "nome": "Auto" }),
        };
        let modello = match a["modello_predefinito_id"].as_str() {
            Some(m) => una_json(c, "SELECT id, etichetta FROM modello WHERE id = ?1", [m])?
                .unwrap_or_else(|| json!({ "id": m, "etichetta": m })),
            None => Value::Null,
        };
        let mid = a["macchina_predefinita_id"]
            .as_str()
            .unwrap_or(server_id)
            .to_string();
        let macchina = match una_json(
            c,
            "SELECT id, nome, genere, stato FROM macchina WHERE id = ?1",
            [&mid],
        )? {
            Some(mut m) => {
                m["in_linea"] = Value::Bool(mid == server_id || connesse.contains(&mid));
                m
            }
            None => Value::Null,
        };
        let routine = tutte_json(
            c,
            "SELECT id, nome, attivatore_tipo, attivatore_json, prossima_ms FROM routine
             WHERE assistente_id = ?1 AND abilitata = 1 ORDER BY nome",
            [&id],
        )?;
        let ultimo: Option<i64> = c
            .query_row(
                "SELECT MAX(t.iniziato_ms) FROM turno t JOIN sessione s ON s.id = t.sessione_id
                 WHERE s.assistente_id = ?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap_or(None);
        v.push(json!({
            "id": id, "nome": a["nome"], "menzione": a["menzione"], "titolo": a["titolo"],
            "descrizione": a["descrizione"], "avatar_url": a["avatar_url"],
            "provider": provider, "modello": modello, "macchina": macchina,
            "usa_chrome": a["usa_chrome"].as_i64() == Some(1),
            "routine_attive": routine, "ultimo_turno_ms": ultimo,
            // --- 2.H ---
            "computer": a["computer"],
        }));
    }
    Ok(v)
}

/// Quando gira una routine, a parole.
pub fn quando(r: &Value) -> String {
    let j = &r["attivatore_json"];
    match r["attivatore_tipo"].as_str() {
        Some("cron") => format!("cron {}", j["expression"].as_str().unwrap_or("")),
        Some("ogni_secondi") => {
            format!("ogni {} secondi", j["every_seconds"].as_i64().unwrap_or(0))
        }
        Some("evento") => format!("all'evento {}", j["evento"].as_str().unwrap_or("")),
        _ => "a mano".into(),
    }
}

pub async fn elenca(State(s): State<Stato>) -> RisHttp<Value> {
    let (server, connesse) = (
        s.servizi.macchina.id.clone(),
        s.motore.macchine().connesse(),
    );
    json_db(&s, move |c| {
        Ok(json!({ "assistenti": squadra(c, &server, &connesse)? }))
    })
    .await
}
