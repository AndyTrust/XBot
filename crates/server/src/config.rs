//! Impostazioni da variabili d'ambiente `XBOT_*`.

use std::net::SocketAddr;
use std::path::PathBuf;

use xbot_core::{Errore, Risposta, SegretoTesto};

use crate::cliente::Rete;

#[derive(Debug, Clone)]
pub struct Config {
    /// `XBOT_DB`: percorso del file SQLite.
    pub db: String,
    /// `XBOT_ASCOLTO`: indirizzo e porta.
    pub ascolto: SocketAddr,
    /// `XBOT_WEB`: cartella dell'interfaccia.
    pub web: PathBuf,
    /// `XBOT_SEGRETO_COOKIE`: base64, almeno 32 byte. Se manca si genera e si salva.
    pub segreto_cookie: Option<SegretoTesto>,
    /// `XBOT_SESSIONI`: dove nascono le cartelle di lavoro vuote delle sessioni
    /// (predefinita `<XBOT_DATI>/sessioni`, vedi `main::servi`).
    pub sessioni: Option<PathBuf>,
    /// `XBOT_COOKIE_SICURO`: `1` cookie sempre `Secure` (dietro un proxy HTTPS anche con
    /// ascolto su 127.0.0.1), `0` mai; se manca vale la regola dell'indirizzo d'ascolto.
    pub cookie_sicuro_forzato: Option<bool>,
    /// `XBOT_PROXY_FIDATO`: IP o CIDR separati da virgola da cui si accetta `X-Forwarded-For`.
    pub proxy_fidati: Vec<Rete>,
}

impl Config {
    pub fn da_ambiente() -> Risposta<Config> {
        let leggi = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        let ascolto = leggi("XBOT_ASCOLTO").unwrap_or_else(|| "127.0.0.1:8787".into());
        let ascolto = ascolto
            .parse()
            .map_err(|e| Errore::non_valida(format!("XBOT_ASCOLTO non valido ({ascolto}): {e}")))?;
        let cookie_sicuro_forzato = match leggi("XBOT_COOKIE_SICURO").as_deref() {
            None => None,
            Some("1") => Some(true),
            Some("0") => Some(false),
            Some(v) => {
                return Err(Errore::non_valida(format!(
                    "XBOT_COOKIE_SICURO vuole 1 o 0, non «{v}»"
                )));
            }
        };
        let proxy_fidati = match leggi("XBOT_PROXY_FIDATO") {
            Some(v) => Rete::leggi_lista(&v)?,
            None => Vec::new(),
        };
        Ok(Config {
            db: leggi("XBOT_DB").unwrap_or_else(|| "xbot.sqlite".into()),
            ascolto,
            web: leggi("XBOT_WEB").map_or_else(|| PathBuf::from("web"), PathBuf::from),
            segreto_cookie: leggi("XBOT_SEGRETO_COOKIE").map(SegretoTesto::from),
            sessioni: leggi("XBOT_SESSIONI").map(PathBuf::from),
            cookie_sicuro_forzato,
            proxy_fidati,
        })
    }

    /// Il cookie è `Secure` fuori da localhost, o sempre/mai con `XBOT_COOKIE_SICURO`.
    pub fn cookie_sicuro(&self) -> bool {
        self.cookie_sicuro_forzato
            .unwrap_or_else(|| !self.ascolto.ip().is_loopback())
    }
}
