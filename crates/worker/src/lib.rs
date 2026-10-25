//! `xbot-worker`: il lavoratore Mac di XBot (Fase 2.E, S-C). Si collega al server con
//! `enroll`, poi `run` tiene il canale WebSocket e esegue sul Mac i turni dei client che stanno
//! solo qui (Cursor, Codex, Gemini) e di Claude quando serve Chrome o i file del Mac.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod accessi;
pub mod canale;
pub mod credenziale;
pub mod esecuzione;
pub mod giornale;
pub mod iscrizione;
pub mod launchagent;
pub mod permessi;
pub mod protocollo;
/// 2.K: la prova dei collegamenti che girano qui.
pub mod prova;
pub mod scoperta;

use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::mpsc;

/// La versione del lavoratore (`--versione`, `versione_lavoratore`).
pub const VERSIONE: &str = env!("CARGO_PKG_VERSION");

/// Dove i lavori mandano i loro frame: (lavoro_id, corpo). Il canale li mette nel giornale.
pub type Uscita = mpsc::Sender<(String, protocollo::Corpo)>;

/// La configurazione di `run`.
#[derive(Debug, Clone)]
pub struct Config {
    /// `~/.xbot/worker` (o `XBOT_WORKER_CARTELLA`): credenziale, giornale, turni, socket.
    pub cartella: PathBuf,
    /// Quanti turni insieme.
    pub posti: u32,
    /// Accetta il provider `finto` (solo prove).
    pub consenti_finto: bool,
    pub battito: Duration,
    /// Le cartelle di lavoro vuote delle sessioni (`sessioni` accanto a `cartella`).
    pub sessioni: PathBuf,
    /// Questo eseguibile (ponte dei permessi e involucro per Chrome).
    pub programma: PathBuf,
    /// I client già noti (prove); `None` = scoperta nel `PATH`.
    pub provider: Option<Vec<protocollo::ProviderMacchina>>,
}

impl Config {
    pub fn predefinita() -> Self {
        Self {
            cartella: credenziale::cartella_predefinita(),
            posti: 2,
            consenti_finto: false,
            battito: Duration::from_secs(20),
            sessioni: credenziale::cartella_sessioni(&credenziale::cartella_predefinita()),
            programma: std::env::current_exe().unwrap_or_else(|_| PathBuf::from("xbot-worker")),
            provider: None,
        }
    }
}

/// Il fornitore di cifratura di rustls (nel grafo ci sono sia `ring` sia `aws-lc-rs`).
pub fn prepara_tls() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}
