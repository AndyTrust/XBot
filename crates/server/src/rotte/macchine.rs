//! `/macchine`: elenco, biglietto per collegarne un'altra, lavori, permesso di esecuzione.

use axum::extract::{Path, State};
use axum::http::HeaderMap;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::{Value, json};
use xbot_core::{PermessoEsecuzione, ora_ms};
use xbot_db::macchine as repo;

use super::json_db;
use crate::errore_http::{Corpo, Domanda, RisHttp};
use crate::stato::Stato;

/// Un biglietto vale 15 minuti.
pub const VITA_BIGLIETTO_MS: i64 = 15 * 60 * 1000;

pub async fn elenca(State(s): State<Stato>) -> RisHttp<Value> {
    // --- S-B (2.E) --- `provider[]` del server dal suo registro, `in_linea` dal canale.
    let hub = s.motore.macchine().clone();
    let (server, ids, connesse) = (
        s.servizi.macchina.id.clone(),
        s.registro.ids(),
        hub.connesse(),
    );
    json_db(&s, move |c| {
        let mut v = repo::elenca(c)?;
        for m in &mut v {
            let id = m["id"].as_str().unwrap_or("").to_string();
            if id == server {
                let locali: Vec<Value> = xbot_db::provider::elenca(c)?
                    .into_iter()
                    .filter(|p| p["id"].as_str().is_some_and(|i| ids.iter().any(|x| x == i)))
                    .map(|p| {
                        let modelli =
                            xbot_db::provider::modelli(c, p["id"].as_str()).unwrap_or_default();
                        json!({ "id": p["id"], "nome": p["nome"], "programma": p["programma"],
                                "versione": null, "visto_ms": null, "modelli": modelli })
                    })
                    .collect();
                m["provider"] = Value::Array(locali);
                m["in_linea"] = Value::Bool(true);
                m["e_il_server"] = Value::Bool(true);
            } else {
                m["in_linea"] = Value::Bool(connesse.contains(&id));
                m["posti_liberi"] = json!(hub.posti_liberi(&id));
                m["e_il_server"] = Value::Bool(false);
            }
        }
        Ok(json!({ "macchine": v }))
    })
    .await
}

/// `{biglietto, comando, scade_ms, nota}`: il comando da lanciare sull'altra macchina.
/// Il lavoratore (`xbot-worker`) arriva con la Fase 2.E.
pub async fn biglietto(State(s): State<Stato>, h: HeaderMap) -> RisHttp<Value> {
    let byte: [u8; 24] = rand::random();
    let biglietto = URL_SAFE_NO_PAD.encode(byte);
    let hash = crate::accesso::hash_token(&biglietto);
    let url = s.servizi.url_pubblico.clone().unwrap_or_else(|| {
        let host = h
            .get("host")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("localhost:8787");
        let schema = if h.get("x-forwarded-proto").and_then(|v| v.to_str().ok()) == Some("https") {
            "https"
        } else {
            "http"
        };
        format!("{schema}://{host}")
    });
    let scade = ora_ms() + VITA_BIGLIETTO_MS;
    json_db(&s, move |c| {
        repo::crea_biglietto(c, &hash, scade)?;
        Ok(json!({
            "biglietto": biglietto,
            "comando": format!("xbot-worker enroll {url} {biglietto}"),
            "scade_ms": scade,
            "scarica": "/api/v1/scarica/xbot-worker-macos",
        }))
    })
    .await
}

#[derive(Deserialize)]
pub struct Limite {
    pub limite: Option<i64>,
}

pub async fn lavori(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Domanda(l): Domanda<Limite>,
) -> RisHttp<Value> {
    let e_il_server = id == s.servizi.macchina.id;
    json_db(&s, move |c| {
        repo::lavori(c, &id, l.limite.unwrap_or(20), e_il_server)
    })
    .await
}

pub async fn permesso(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    json_db(&s, move |c| {
        let m = repo::leggi(c, &id)?;
        Ok(json!({ "permesso_esecuzione": m["permesso_esecuzione"] }))
    })
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpoPermesso {
    pub permesso_esecuzione: PermessoEsecuzione,
}

pub async fn scrivi_permesso(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Corpo(b): Corpo<CorpoPermesso>,
) -> RisHttp<Value> {
    json_db(&s, move |c| {
        let m = repo::imposta_permesso(c, &id, b.permesso_esecuzione)?;
        Ok(json!({ "permesso_esecuzione": m["permesso_esecuzione"] }))
    })
    .await
}

// --- S-B (2.E) --- collegare (biglietto), rimuovere, scaricare il lavoratore.

#[derive(Deserialize)]
pub struct CorpoCollega {
    pub biglietto: String,
    #[serde(flatten)]
    pub macchina: repo::NuovaMacchina,
}

/// `POST /macchine/collega` (senza accesso: vale il biglietto) → `{macchina_id, credenziale}`.
/// La credenziale (32 byte casuali) si mostra solo qui; nel database resta l'hash.
pub async fn collega(State(s): State<Stato>, Corpo(b): Corpo<CorpoCollega>) -> RisHttp<Value> {
    let hash_biglietto = crate::accesso::hash_token(b.biglietto.trim());
    let byte: [u8; 32] = rand::random();
    let credenziale = URL_SAFE_NO_PAD.encode(byte);
    let hash_cred = crate::accesso::hash_token(&credenziale);
    let m = b.macchina;
    json_db(&s, move |c| {
        let id = repo::collega(c, &hash_biglietto, &m, &hash_cred)?;
        xbot_db::eventi::scrivi(
            c,
            "macchina_registrata",
            &json!({ "macchina_id": id, "nome": m.nome }),
        )?;
        Ok(json!({ "macchina_id": id, "credenziale": credenziale }))
    })
    .await
}

/// `DELETE /macchine/{id}`: macchina rimossa, credenziale revocata, socket chiuso. Il server
/// stesso non si rimuove.
pub async fn elimina(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    if id == s.servizi.macchina.id {
        return Err(xbot_core::Errore::non_valida("il server non si rimuove").into());
    }
    let i2 = id.clone();
    let r = json_db(&s, move |c| {
        repo::rimuovi(c, &i2)?;
        xbot_db::eventi::scrivi(c, "macchina_rimossa", &json!({ "macchina_id": i2 }))?;
        Ok(json!({ "macchina_id": i2, "stato": "rimossa" }))
    })
    .await?;
    s.motore.macchine().caccia(&id);
    Ok(r)
}

/// `GET /scarica/xbot-worker-macos`: il binario del lavoratore dalla cartella
/// `XBOT_SCARICA` (404 se manca).
pub async fn scarica_lavoratore() -> axum::response::Response {
    use axum::response::IntoResponse;
    let nome = "xbot-worker-macos";
    let cartella = std::env::var("XBOT_SCARICA")
        .ok()
        .filter(|v| !v.trim().is_empty());
    let Some(cartella) = cartella else {
        return crate::errore_http::ErroreHttp(xbot_core::Errore::nuovo(
            xbot_core::Codice::NonTrovato,
            "il lavoratore non è disponibile su questo server",
        ))
        .into_response();
    };
    match tokio::fs::read(std::path::Path::new(&cartella).join(nome)).await {
        Ok(b) => (
            [
                (axum::http::header::CONTENT_TYPE, "application/octet-stream"),
                (
                    axum::http::header::CONTENT_DISPOSITION,
                    "attachment; filename=\"xbot-worker\"",
                ),
            ],
            b,
        )
            .into_response(),
        Err(_) => crate::errore_http::ErroreHttp(xbot_core::Errore::nuovo(
            xbot_core::Codice::NonTrovato,
            "il lavoratore non è disponibile su questo server",
        ))
        .into_response(),
    }
}
// --- fine S-B ---

// --- S-B (2.E) ---
/// `POST /turni/{id}/sulla-vps`: il turno in attesa del Mac riparte qui con `claude_code`.
/// Risponde con il turno aggiornato (provider e macchina nuovi); 409 se non è in attesa.
pub async fn sulla_vps(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    s.motore.macchine().sulla_vps(&id)?;
    let tid = id.clone();
    // Il motore riscrive il turno appena si sveglia: si aspetta al massimo un paio di secondi.
    for _ in 0..100 {
        let t2 = tid.clone();
        let (t, ripartito) =
            s.db.esegui(move |c| {
                let t = xbot_db::turni::leggi(c, &t2)?;
                let l = xbot_db::lavori::per_turno(c, &t2)?;
                let ripartito = match l {
                    Some(l) => xbot_db::lavori::leggi(c, &l)?["stato"] == "annullato",
                    None => true,
                };
                Ok((t, ripartito))
            })
            .await?;
        if ripartito {
            return Ok(axum::Json(con_turno_id(&t)));
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let t = s.db.esegui(move |c| xbot_db::turni::leggi(c, &tid)).await?;
    Ok(axum::Json(con_turno_id(&t)))
}

/// Il turno con anche `turno_id` (= `id`).
fn con_turno_id(t: &xbot_db::turni::Turno) -> Value {
    let mut v = serde_json::to_value(t).unwrap_or_default();
    v["turno_id"] = v["id"].clone();
    v
}
// --- fine S-B ---
