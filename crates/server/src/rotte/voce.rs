//! `/trascrivi`, `/sintesi`, `/voci` (Fase 2.D S2).

use axum::Json;
use axum::body::Body;
use axum::extract::{Multipart, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use xbot_core::Errore;
use xbot_db::impostazioni;

use crate::errore_http::{Corpo, ErroreHttp, RisHttp};
use crate::stato::Stato;
use crate::voce;

/// L'audio più grande accettato (25 MB).
pub const LIMITE_AUDIO: usize = 25 * 1024 * 1024;
/// Il testo più lungo da sintetizzare.
pub const LIMITE_TESTO: usize = 5000;

fn estensione(nome: Option<&str>, tipo: Option<&str>) -> &'static str {
    let n = nome.unwrap_or_default().to_lowercase();
    let t = tipo.unwrap_or_default();
    for (e, chiave) in [
        ("wav", "wav"),
        ("mp4", "mp4"),
        ("m4a", "m4a"),
        ("ogg", "ogg"),
        ("mp3", "mpeg"),
        ("webm", "webm"),
    ] {
        if n.ends_with(&format!(".{e}")) || t.contains(chiave) {
            return e;
        }
    }
    "webm"
}

/// Un corpo oltre il limite → 413 `corpo_troppo_grande`; il resto 400.
fn errore_multipart(e: axum::extract::multipart::MultipartError) -> Errore {
    if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
        Errore::nuovo(xbot_core::Codice::CorpoTroppoGrande, "audio oltre 25 MB")
    } else {
        Errore::non_valida("multipart non leggibile")
    }
}

/// `POST /trascrivi`: multipart `audio` + `lingua?` → `{testo, durata_ms, motore, modello}`.
pub async fn trascrivi(State(s): State<Stato>, mut m: Multipart) -> RisHttp<Value> {
    let mut audio: Option<(Vec<u8>, &'static str)> = None;
    let mut lingua = "it".to_string();
    while let Some(campo) = m.next_field().await.map_err(errore_multipart)? {
        match campo.name() {
            Some("audio") => {
                let ext = estensione(campo.file_name(), campo.content_type());
                let b = campo.bytes().await.map_err(errore_multipart)?;
                if b.len() > LIMITE_AUDIO {
                    return Err(Errore::nuovo(
                        xbot_core::Codice::CorpoTroppoGrande,
                        "audio oltre 25 MB",
                    )
                    .into());
                }
                audio = Some((b.to_vec(), ext));
            }
            Some("lingua") => {
                let t = campo.text().await.unwrap_or_default();
                let t = t.trim();
                if !t.is_empty()
                    && t.len() <= 8
                    && t.chars().all(|c| c.is_ascii_alphabetic() || c == '-')
                {
                    lingua = t.to_string();
                }
            }
            _ => {}
        }
    }
    let (byte, ext) = audio.ok_or_else(|| Errore::non_valida("manca il campo audio"))?;
    if byte.is_empty() {
        return Err(Errore::non_valida("l'audio è vuoto").into());
    }
    let dir = std::env::temp_dir().join(format!("xbot-voce-{}", xbot_core::nuovo_id()));
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| Errore::interno(format!("cartella temporanea: {e}")))?;
    let file = dir.join(format!("audio.{ext}"));
    let scritto = tokio::fs::write(&file, &byte).await;
    let esito = match scritto {
        Ok(()) => voce::trascrivi(&s.motore.s2().voce(), &file, &lingua).await,
        Err(e) => Err(Errore::interno(format!("file audio: {e}"))),
    };
    let _ = tokio::fs::remove_dir_all(&dir).await;
    let v = esito?;
    tracing::info!(
        byte = byte.len(),
        caratteri = v["testo"].as_str().map_or(0, |t| t.chars().count()),
        "trascrizione"
    );
    Ok(Json(v))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpoSintesi {
    pub testo: String,
    pub voce_id: Option<String>,
    pub velocita: Option<f64>,
}

/// `POST /sintesi` → `audio/mpeg` in flusso.
pub async fn sintesi(
    State(s): State<Stato>,
    Corpo(b): Corpo<CorpoSintesi>,
) -> Result<Response, ErroreHttp> {
    if b.testo.trim().is_empty() {
        return Err(Errore::non_valida("il testo è vuoto").into());
    }
    if b.testo.chars().count() > LIMITE_TESTO {
        return Err(Errore::nuovo(
            xbot_core::Codice::TestoTroppoLungo,
            format!("al massimo {LIMITE_TESTO} caratteri"),
        )
        .into());
    }
    let cfg = s.motore.s2().voce();
    let detto = voce::normalizza(&cfg, &b.testo).await;
    let r = voce::sintesi(
        &cfg,
        &detto,
        b.voce_id.as_deref().filter(|v| !v.is_empty()),
        b.velocita,
    )
    .await?;
    tracing::info!(caratteri = detto.chars().count(), "sintesi");
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "audio/mpeg"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        Body::from_stream(r.bytes_stream()),
    )
        .into_response())
}

/// `GET /voci` → `{"voci":[{id, nome}]}`: la voce del bot orchestratore (`XBOT_NOME`) e quelle
/// in `voce.elenco`.
pub async fn voci(State(s): State<Stato>) -> RisHttp<Value> {
    let altre =
        s.db.esegui(|c| impostazioni::leggi(c, "voce.elenco"))
            .await?;
    let mut v = Vec::new();
    if let Some(id) = s.motore.s2().voce().fish_voce.clone() {
        v.push(json!({ "id": id, "nome": xbot_core::nome_bot() }));
    }
    if let Some(Value::Array(a)) = altre {
        for x in a {
            if x["id"].is_string() && !v.iter().any(|y| y["id"] == x["id"]) {
                v.push(json!({ "id": x["id"], "nome": x["nome"].as_str().unwrap_or("voce") }));
            }
        }
    }
    Ok(Json(json!({ "voci": v })))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn estensioni() {
        assert_eq!(estensione(Some("a.WAV"), None), "wav");
        assert_eq!(estensione(None, Some("audio/webm;codecs=opus")), "webm");
        assert_eq!(estensione(None, Some("audio/mpeg")), "mp3");
        assert_eq!(estensione(None, None), "webm");
    }
}
