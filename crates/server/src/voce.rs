//! La voce (Fase 2.D S2): trascrizione con faster-whisper (script Python `voce/trascrivi.py`),
//! normalizzazione «tutto per intero» (`voce/normalizza.py`) e sintesi con fish.audio (flusso
//! audio in streaming).
//!
//! Variabili: `XBOT_VOCE_PYTHON` (predefinito `python3`), `XBOT_VOCE_CARTELLA` (dove stanno
//! gli script, predefinita `voce`), `XBOT_VOCE_TRASCRIVI`, `XBOT_VOCE_NORMALIZZA`,
//! `XBOT_VOCE_MODELLO` (lo legge lo script), `FISH_API_KEY`, `XBOT_FISH_VOCE`,
//! `XBOT_FISH_MODELLO` (predefinito `s2.1-pro`), `XBOT_FISH_URL` (per le prove).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};
use xbot_core::{Codice, Errore, Risposta, SegretoTesto};

use crate::mcp_interno::strumenti::programma;

pub const FISH_URL: &str = "https://api.fish.audio/v1/tts";
pub const MODELLO_GRATUITO: &str = "s2.1-pro-free";
pub const TEMPO_TRASCRIZIONE: Duration = Duration::from_secs(120);

fn variabile(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.trim().is_empty())
}

/// Lancia un programma senza shell; restituisce stdout e stderr qualunque sia l'uscita.
async fn uscita(
    prog: &str,
    argomenti: &[String],
    tempo: Duration,
) -> Result<(String, String), String> {
    let mut cmd = tokio::process::Command::new(prog);
    cmd.args(argomenti)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let o = tokio::time::timeout(tempo, cmd.output())
        .await
        .map_err(|_| "tempo scaduto".to_string())?
        .map_err(|e| e.to_string())?;
    Ok((
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    ))
}

/// La configurazione della voce: dall'ambiente all'avvio, sostituibile nelle prove.
#[derive(Debug, Clone)]
pub struct ConfigVoce {
    pub python: String,
    pub trascrivi: PathBuf,
    pub normalizza: PathBuf,
    pub fish_url: String,
    pub fish_voce: Option<String>,
    pub fish_modello: String,
    pub fish_chiave: Option<SegretoTesto>,
}

impl ConfigVoce {
    pub fn da_ambiente() -> Self {
        let cartella =
            PathBuf::from(variabile("XBOT_VOCE_CARTELLA").unwrap_or_else(|| "voce".into()));
        Self {
            python: variabile("XBOT_VOCE_PYTHON").unwrap_or_else(|| "python3".into()),
            trascrivi: variabile("XBOT_VOCE_TRASCRIVI")
                .map_or_else(|| cartella.join("trascrivi.py"), PathBuf::from),
            normalizza: variabile("XBOT_VOCE_NORMALIZZA")
                .map_or_else(|| cartella.join("normalizza.py"), PathBuf::from),
            fish_url: variabile("XBOT_FISH_URL").unwrap_or_else(|| FISH_URL.into()),
            fish_voce: variabile("XBOT_FISH_VOCE"),
            fish_modello: variabile("XBOT_FISH_MODELLO").unwrap_or_else(|| "s2.1-pro".into()),
            fish_chiave: variabile("FISH_API_KEY").map(SegretoTesto::nuovo),
        }
    }
}

/// Trascrive un file audio: `{testo, durata_ms, motore, modello}`.
pub async fn trascrivi(cfg: &ConfigVoce, file: &Path, lingua: &str) -> Risposta<Value> {
    let s = cfg.trascrivi.clone();
    if !s.is_file() {
        return Err(Errore::nuovo(
            Codice::VoceNonConfigurata,
            format!("script di trascrizione assente: {}", s.display()),
        ));
    }
    let arg = vec![
        s.to_string_lossy().into_owned(),
        file.to_string_lossy().into_owned(),
        lingua.to_string(),
    ];
    // Lo script scrive sempre una riga JSON, anche quando esce con errore: i dettagli (percorsi,
    // eccezioni) vanno solo nel log, al client un codice.
    let fallita = || Errore::nuovo(Codice::VoceFallita, "trascrizione non riuscita");
    let (out, err) = uscita(&cfg.python, &arg, TEMPO_TRASCRIZIONE)
        .await
        .map_err(|e| {
            tracing::warn!(errore = %e, "trascrizione: processo non partito");
            fallita()
        })?;
    let v: Value = serde_json::from_str(out.trim()).map_err(|_| {
        tracing::warn!(stderr = %err.chars().take(300).collect::<String>(), "trascrizione: uscita non in JSON");
        fallita()
    })?;
    if let Some(e) = v.get("errore").and_then(Value::as_str) {
        if e == "audio_non_valido" {
            return Err(Errore::nuovo(
                Codice::AudioNonValido,
                "il file non è un audio leggibile",
            ));
        }
        tracing::warn!(errore = %e, "trascrizione fallita");
        return Err(fallita());
    }
    Ok(json!({
        "testo": v["testo"].as_str().unwrap_or_default().trim(),
        "durata_ms": v["durata_ms"].as_i64().unwrap_or(0),
        "motore": "faster-whisper",
        "modello": v.get("modello").cloned().unwrap_or(Value::Null),
    }))
}

/// Il testo come va detto (numeri, orari, date, sigle per intero). Se lo script non c'è o
/// fallisce, il testo resta com'è (con un avviso nel log).
pub async fn normalizza(cfg: &ConfigVoce, testo: &str) -> String {
    let s = cfg.normalizza.clone();
    if !s.is_file() {
        tracing::warn!(script = %s.display(), "normalizzazione della voce assente");
        return testo.to_string();
    }
    match programma(
        &cfg.python,
        &[s.to_string_lossy().into_owned()],
        Some(testo),
        Duration::from_secs(15),
    )
    .await
    {
        Ok(t) if !t.trim().is_empty() => t.trim().to_string(),
        Ok(_) => testo.to_string(),
        Err(e) => {
            tracing::warn!(errore = %e, "normalizzazione della voce fallita: testo com'è");
            testo.to_string()
        }
    }
}

/// Chiama fish.audio e restituisce la risposta (200) da passare in flusso. 402 con il modello a
/// pagamento → un secondo tentativo con quello gratuito; 402 anche lì → `credito_voce_finito`.
pub async fn sintesi(
    cfg: &ConfigVoce,
    testo: &str,
    voce: Option<&str>,
    velocita: Option<f64>,
) -> Risposta<reqwest::Response> {
    let chiave = cfg.fish_chiave.clone().ok_or_else(|| {
        Errore::nuovo(Codice::VoceNonConfigurata, "manca FISH_API_KEY sul server")
    })?;
    let voce = voce
        .map(str::to_string)
        .or_else(|| cfg.fish_voce.clone())
        .ok_or_else(|| {
            Errore::nuovo(Codice::VoceNonConfigurata, "manca la voce (XBOT_FISH_VOCE)")
        })?;
    let url = cfg.fish_url.clone();
    let modello = cfg.fish_modello.clone();
    let modelli: Vec<String> = if modello == MODELLO_GRATUITO {
        vec![modello]
    } else {
        vec![modello, MODELLO_GRATUITO.into()]
    };
    let corpo = json!({
        "text": testo, "reference_id": voce, "format": "mp3", "latency": "balanced",
        "normalize": true, "prosody": { "speed": velocita.unwrap_or(1.0).clamp(0.5, 2.0) },
    });
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| Errore::interno(format!("client http: {e}")))?;
    for (i, m) in modelli.iter().enumerate() {
        let r = client
            .post(&url)
            .bearer_auth(chiave.esponi())
            .header("model", m)
            .json(&corpo)
            .send()
            .await
            .map_err(|e| {
                Errore::nuovo(
                    Codice::VoceFallita,
                    format!("fish.audio non raggiungibile: {e}"),
                )
            })?;
        let stato = r.status().as_u16();
        if stato == 200 {
            return Ok(r);
        }
        if stato == 402 && i + 1 < modelli.len() {
            tracing::info!(modello = %m, "fish.audio 402 (credito): provo il modello gratuito");
            continue;
        }
        if stato == 402 {
            return Err(Errore::nuovo(
                Codice::CreditoVoceFinito,
                "il credito di fish.audio è finito",
            ));
        }
        let testo: String = r
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(120)
            .collect();
        return Err(Errore::nuovo(
            Codice::VoceFallita,
            format!("fish.audio HTTP {stato}: {testo}"),
        ));
    }
    Err(Errore::nuovo(
        Codice::CreditoVoceFinito,
        "il credito di fish.audio è finito",
    ))
}
