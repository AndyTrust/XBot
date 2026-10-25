//! La credenziale del Mac: chiave P-256 generata a `enroll`, salvata con la credenziale del
//! server in `<cartella>/credenziale.json` con permessi 0600. Mai nel `Debug`, mai nei log.

use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use p256::SecretKey;
use p256::pkcs8::{EncodePrivateKey, EncodePublicKey};
use serde::{Deserialize, Serialize};
use xbot_core::{Errore, Risposta, SegretoTesto};

/// `XBOT_WORKER_CARTELLA`, altrimenti `~/.xbot/worker`.
pub fn cartella_predefinita() -> PathBuf {
    if let Some(p) = std::env::var_os("XBOT_WORKER_CARTELLA").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    xbot_core::cartella_casa().join("worker")
}

/// Le cartelle di lavoro delle sessioni del lavoratore: `XBOT_SESSIONI`, altrimenti
/// `sessioni` accanto alla cartella del lavoratore (`~/.xbot/sessioni` col predefinito).
pub fn cartella_sessioni(cartella: &Path) -> PathBuf {
    if let Some(p) = std::env::var_os("XBOT_SESSIONI").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    cartella
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .map_or_else(|| cartella.join("sessioni"), |d| d.join("sessioni"))
}

pub fn file_credenziale(cartella: &Path) -> PathBuf {
    cartella.join("credenziale.json")
}

/// Una coppia di chiavi nuova: (privata PKCS8 DER in base64, pubblica SPKI DER in base64).
pub fn nuova_chiave() -> Risposta<(SegretoTesto, String)> {
    let segreta = loop {
        let byte: [u8; 32] = rand::random();
        if let Ok(k) = SecretKey::from_slice(&byte) {
            break k;
        }
    };
    let privata = segreta
        .to_pkcs8_der()
        .map_err(|e| Errore::interno(format!("chiave privata: {e}")))?;
    let pubblica = segreta
        .public_key()
        .to_public_key_der()
        .map_err(|e| Errore::interno(format!("chiave pubblica: {e}")))?;
    Ok((
        SegretoTesto::nuovo(STANDARD.encode(privata.as_bytes())),
        STANDARD.encode(pubblica.as_bytes()),
    ))
}

/// Quello che resta sul Mac dopo `enroll`.
#[derive(Clone, Serialize, Deserialize)]
pub struct Credenziale {
    pub url: String,
    pub macchina_id: String,
    pub credenziale: String,
    /// PKCS8 DER in base64.
    pub chiave_privata: String,
    pub chiave_pubblica: String,
    pub nome: String,
    pub creata_ms: i64,
}

impl std::fmt::Debug for Credenziale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credenziale")
            .field("url", &self.url)
            .field("macchina_id", &self.macchina_id)
            .field("nome", &self.nome)
            .finish_non_exhaustive()
    }
}

impl Credenziale {
    /// L'indirizzo del canale: `wss://<host>/api/v1/macchine/<id>/canale`.
    pub fn url_canale(&self) -> String {
        let base = self.url.trim_end_matches('/');
        let base = if let Some(r) = base.strip_prefix("https://") {
            format!("wss://{r}")
        } else if let Some(r) = base.strip_prefix("http://") {
            format!("ws://{r}")
        } else {
            base.to_string()
        };
        format!("{base}/api/v1/macchine/{}/canale", self.macchina_id)
    }

    pub fn leggi(file: &Path) -> Risposta<Self> {
        let t = std::fs::read_to_string(file).map_err(|e| {
            Errore::interno(format!(
                "{}: {e} (prima: xbot-worker enroll <url> <biglietto>)",
                file.display()
            ))
        })?;
        serde_json::from_str(&t)
            .map_err(|e| Errore::interno(format!("{}: illeggibile: {e}", file.display())))
    }

    pub fn salva(&self, file: &Path) -> Risposta<()> {
        let t = serde_json::to_vec_pretty(self).map_err(|e| Errore::interno(e.to_string()))?;
        scrivi_privato(file, &t)
    }
}

/// Scrive un file con permessi 0600 fin dalla creazione (cartella 0700 se manca).
pub fn scrivi_privato(file: &Path, dati: &[u8]) -> Risposta<()> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
    let errore = |e: std::io::Error| Errore::interno(format!("{}: {e}", file.display()));
    if let Some(d) = file.parent()
        && !d.exists()
    {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(d)
            .map_err(errore)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(file)
        .map_err(errore)?;
    // Un file che esisteva già può avere permessi più larghi.
    f.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(errore)?;
    f.write_all(dati).map_err(errore)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn chiave_e_url() {
        let (privata, pubblica) = nuova_chiave().unwrap();
        assert!(privata.lunghezza() > 100 && pubblica.len() > 100);
        let c = Credenziale {
            url: "https://xbot.esempio/".into(),
            macchina_id: "M1".into(),
            credenziale: "segreta".into(),
            chiave_privata: privata.esponi().into(),
            chiave_pubblica: pubblica,
            nome: "mac".into(),
            creata_ms: 0,
        };
        assert_eq!(
            c.url_canale(),
            "wss://xbot.esempio/api/v1/macchine/M1/canale"
        );
        assert!(!format!("{c:?}").contains("segreta"));
    }
}
