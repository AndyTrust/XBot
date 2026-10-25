//! `xbot-worker enroll <url> <biglietto>`: chiave nuova, `POST /api/v1/macchine/collega`,
//! credenziale salvata 0600.

use std::path::Path;

use serde::Deserialize;
use serde_json::json;
use xbot_core::{Errore, Risposta, ora_ms};

use crate::credenziale::{Credenziale, file_credenziale, nuova_chiave};
use crate::protocollo::ProviderMacchina;

/// Le capacità del Mac.
pub const CAPACITA: [&str; 3] = ["turni", "chrome", "file"];

#[derive(Deserialize)]
struct RispostaCollega {
    macchina_id: String,
    credenziale: String,
}

pub struct Iscrizione<'a> {
    pub url: &'a str,
    pub biglietto: &'a str,
    pub nome: &'a str,
    pub posti: u32,
    pub provider: Vec<ProviderMacchina>,
}

/// Collega il Mac e salva la credenziale in `<cartella>/credenziale.json`.
pub async fn iscrivi(cartella: &Path, i: Iscrizione<'_>) -> Risposta<Credenziale> {
    let url = i.url.trim_end_matches('/').to_string();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(Errore::non_valida(format!("indirizzo non valido: {url}")));
    }
    let (privata, pubblica) = nuova_chiave()?;
    let corpo = json!({
        "biglietto": i.biglietto,
        "nome": i.nome,
        "genere": "mac",
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "versione_lavoratore": crate::VERSIONE,
        "chiave_pubblica": pubblica,
        "posti": i.posti,
        "capacita": CAPACITA,
        "provider": i.provider,
    });
    let cliente = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| Errore::interno(format!("client HTTP: {e}")))?;
    let r = cliente
        .post(format!("{url}/api/v1/macchine/collega"))
        .json(&corpo)
        .send()
        .await
        .map_err(|e| Errore::interno(format!("{url}: {e}")))?;
    let stato = r.status();
    let testo = r.text().await.unwrap_or_default();
    if !stato.is_success() {
        let breve: String = testo.chars().take(300).collect();
        return Err(Errore::non_valida(format!(
            "il server ha risposto {stato}: {breve}"
        )));
    }
    let v: RispostaCollega = serde_json::from_str(&testo)
        .map_err(|e| Errore::interno(format!("risposta del server illeggibile: {e}")))?;
    let c = Credenziale {
        url,
        macchina_id: v.macchina_id,
        credenziale: v.credenziale,
        chiave_privata: privata.esponi().to_string(),
        chiave_pubblica: pubblica,
        nome: i.nome.to_string(),
        creata_ms: ora_ms(),
    };
    c.salva(&file_credenziale(cartella))?;
    Ok(c)
}
