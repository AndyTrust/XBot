//! Fase 2.E-bis S-B: la chiamata vocale e il voto stanno sul server, così si vedono da ogni
//! dispositivo. `POST /sessioni/{id}/chiamate` scrive la riga `evento` «Chat vocale · durata»;
//! `POST /righe/{riga_id}/voto` (o `/sessioni/{id}/righe/{riga_id}/voto`) mette `voto` nel
//! corpo di una riga chiamata o di un messaggio dell'assistente e la riscrive nel flusso.

use axum::Json;
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use xbot_core::{Codice, Errore, GenereRiga, Ruolo};
use xbot_db::righe;

use crate::bus::Segnale;
use crate::errore_http::{Corpo, RisHttp};
use crate::stato::Stato;

/// Il tipo del corpo della riga `evento` di una chiamata.
pub const TIPO_CHIAMATA: &str = "chiamata_vocale";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NuovaChiamata {
    pub iniziata_ms: i64,
    pub durata_ms: i64,
    #[serde(default)]
    pub invii: i64,
    #[serde(default)]
    pub risposte: i64,
}

/// `POST /sessioni/{id}/chiamate` → `{riga_id}`.
pub async fn crea(
    State(s): State<Stato>,
    Path(sid): Path<String>,
    Corpo(n): Corpo<NuovaChiamata>,
) -> RisHttp<Value> {
    if n.iniziata_ms < 0 || n.durata_ms < 0 || n.invii < 0 || n.risposte < 0 {
        return Err(
            Errore::non_valida("iniziata_ms, durata_ms, invii e risposte non negativi").into(),
        );
    }
    let corpo = json!({
        "tipo": TIPO_CHIAMATA, "iniziata_ms": n.iniziata_ms, "durata_ms": n.durata_ms,
        "invii": n.invii, "risposte": n.risposte, "voto": null,
    });
    let id = sid.clone();
    let r = s
        .db
        .esegui(move |c| righe::aggiungi(c, &id, GenereRiga::Evento, Some(Ruolo::Sistema), &corpo))
        .await?;
    s.bus.pubblica(
        &sid,
        Segnale::Righe {
            aggiornata_seq: r.aggiornata_seq,
        },
    );
    Ok(Json(json!({ "riga_id": r.id })))
}

/// Il voto dal corpo: `su`, `giu` o `null` (il campo `voto` è obbligatorio).
fn voto(m: &Map<String, Value>) -> Result<Value, Errore> {
    match m.get("voto") {
        Some(Value::Null) => Ok(Value::Null),
        Some(Value::String(v)) if v == "su" || v == "giu" => Ok(json!(v)),
        Some(_) => Err(Errore::non_valida("voto: «su», «giu» o null")),
        None => Err(Errore::non_valida("manca il campo «voto»")),
    }
}

async fn vota(s: Stato, sessione: Option<String>, riga_id: String, v: Value) -> RisHttp<Value> {
    let voto_risposta = v.clone();
    let r =
        s.db.esegui(move |c| {
            let riga = righe::leggi(c, &riga_id)?;
            if sessione
                .as_deref()
                .is_some_and(|sid| sid != riga.sessione_id)
            {
                return Err(Errore::nuovo(Codice::NonTrovato, "riga non trovata"));
            }
            let chiamata = riga.genere == GenereRiga::Evento.come_testo()
                && riga.corpo["tipo"] == TIPO_CHIAMATA;
            let risposta = riga.genere == GenereRiga::Messaggio.come_testo()
                && riga.ruolo.as_deref() == Some(Ruolo::Assistente.come_testo());
            if !chiamata && !risposta {
                return Err(Errore::non_valida(
                    "si vota solo una chiamata vocale o un messaggio dell'assistente",
                ));
            }
            let mut corpo = match riga.corpo {
                Value::Object(o) => Value::Object(o),
                _ => json!({}),
            };
            corpo["voto"] = v;
            righe::riscrivi(c, &riga_id, &corpo)
        })
        .await?;
    s.bus.pubblica(
        &r.sessione_id,
        Segnale::Righe {
            aggiornata_seq: r.aggiornata_seq,
        },
    );
    Ok(Json(json!({
        "riga_id": r.id, "sessione_id": r.sessione_id, "voto": voto_risposta,
        "aggiornata_seq": r.aggiornata_seq,
    })))
}

/// `POST /righe/{riga_id}/voto` `{voto: "su"|"giu"|null}`.
pub async fn voto_riga(
    State(s): State<Stato>,
    Path(riga_id): Path<String>,
    Corpo(m): Corpo<Map<String, Value>>,
) -> RisHttp<Value> {
    let v = voto(&m)?;
    vota(s, None, riga_id, v).await
}

/// `POST /sessioni/{id}/righe/{riga_id}/voto`: come sopra, 404 se la riga è di un'altra sessione.
pub async fn voto_riga_sessione(
    State(s): State<Stato>,
    Path((sid, riga_id)): Path<(String, String)>,
    Corpo(m): Corpo<Map<String, Value>>,
) -> RisHttp<Value> {
    let v = voto(&m)?;
    vota(s, Some(sid), riga_id, v).await
}
