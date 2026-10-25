//! I provider: il trait comune e gli adattatori (`finto` per i test, `claude_code`,
//! `cursor_agent`, `codex`, `gemini`, `grok`).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod accesso;
pub mod caldi;
pub mod claude_code;
pub mod codex;
pub mod codex_quota;
pub mod cursor_agent;
pub mod esterni;
pub mod finto;
pub mod gemini;
pub mod grok;
/// 2.L: i collegamenti del turno per Grok e Cursor.
pub mod mcp_altri;
pub mod prova_mcp;

use std::path::PathBuf;

use async_trait::async_trait;
use serde::Serialize;
use tokio::sync::mpsc;
use xbot_core::{Errore, Id, Ms};

pub use accesso::{Account, ComandoAccesso};
pub use claude_code::ClaudeCode;
pub use codex::Codex;
pub use cursor_agent::CursorAgent;
pub use finto::Finto;
pub use gemini::Gemini;
pub use grok::Grok;

/// 2.L: i client che ricevono i collegamenti nei turni (Claude Code, Grok, Cursor).
pub fn riceve_collegamenti(provider_id: &str) -> bool {
    [claude_code::ID, grok::ID, cursor_agent::ID].contains(&provider_id)
}

/// Un modello offerto da un provider.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Modello {
    pub id: String,
    pub etichetta: String,
    pub dettaglio: Option<String>,
    pub livelli: Vec<String>,
}

/// La quota letta dal client ufficiale.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Quota {
    pub percentuale_usata: f64,
    pub azzera_ms: Option<Ms>,
}

/// Tutto quello che serve per un turno.
#[derive(Debug, Clone, Default)]
pub struct RichiestaTurno {
    pub sessione_id: Id,
    pub sessione_esterna_id: Option<String>,
    pub modello: Option<String>,
    pub livello: Option<String>,
    pub cartella_lavoro: Option<PathBuf>,
    pub testo: String,
    pub allegati: Vec<PathBuf>,
    /// Profilo dell'assistente e memoria.
    pub sistema: Option<String>,
    /// Modo dei permessi del client (`acceptEdits` se manca).
    pub permessi: Option<String>,
    /// Server MCP del turno (Fase 2.D): con lo strumento dei permessi il modo diventa `default`.
    pub mcp: Option<claude_code::mcp::ConfigMcp>,
    /// Variabili d'ambiente del processo del provider (segreti dell'assistente e della cassaforte).
    pub ambiente: Vec<(String, xbot_core::SegretoTesto)>,
    /// Fase 2.G: chi decide le richieste di permesso del client (ACP di grok) quando il permesso
    /// della macchina è `ask`. `None` = negate, con un avviso.
    pub chiedi_permesso: Option<Permessi>,
}

/// Decide se uno strumento del client può partire (lo chiede all'utente).
#[async_trait]
pub trait ChiediPermesso: Send + Sync {
    /// `strumento` alla Claude Code (`Bash`, `Edit`, …), `input` con `command` per Bash.
    /// `Ok` = consentito, `Err(motivo)` = negato.
    async fn chiedi(&self, strumento: &str, input: &serde_json::Value) -> Result<(), String>;
}

/// Una maniglia clonabile su chi decide i permessi.
#[derive(Clone)]
pub struct Permessi(pub std::sync::Arc<dyn ChiediPermesso>);

impl std::fmt::Debug for Permessi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Permessi")
    }
}

/// Quello che il provider racconta durante un turno.
#[derive(Debug, Clone, PartialEq)]
pub enum EventoTurno {
    Avviato {
        sessione_esterna_id: Option<String>,
    },
    TestoDelta(String),
    TestoCompleto(String),
    Ragionamento(String),
    ChiamataStrumento {
        nome: String,
        input_json: serde_json::Value,
    },
    EsitoStrumento {
        nome: String,
        esito_json: serde_json::Value,
        ok: bool,
    },
    Avviso(String),
    Finito {
        token_ingresso: Option<i64>,
        token_uscita: Option<i64>,
        token_cache: Option<i64>,
        /// Il costo come lo dichiara il client, in dollari (`total_cost_usd`).
        costo_usd: Option<f64>,
        sessione_esterna_id: Option<String>,
    },
    /// La quota dell'abbonamento letta dal client: `utilizzo` da 0 a 1.
    Quota {
        finestra: String,
        utilizzo: f64,
        azzera_ms: Option<Ms>,
    },
    Fallito {
        codice: String,
        testo: String,
    },
}

/// La maniglia di un turno avviato: il processo e come fermarlo.
pub struct ManigliaTurno {
    pub pid: Option<u32>,
    pub interrompi: Box<dyn FnOnce() + Send>,
}

impl std::fmt::Debug for ManigliaTurno {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManigliaTurno")
            .field("pid", &self.pid)
            .finish_non_exhaustive()
    }
}

/// Un provider: un client ufficiale lanciato dal motore (o il finto).
#[async_trait]
pub trait Provider: Send + Sync {
    /// `claude_code` | `grok` | `cursor_agent` | `finto`
    fn id(&self) -> &str;
    /// Il nome da mostrare.
    fn nome(&self) -> &str;
    /// Il programma lanciato (per la tabella `provider`).
    fn programma(&self) -> &str;
    async fn modelli(&self) -> Result<Vec<Modello>, Errore>;
    /// `None` = non leggibile dal client (si stima dai turni).
    async fn quota(&self) -> Result<Option<Quota>, Errore>;
    async fn avvia(
        &self,
        richiesta: RichiestaTurno,
        eventi: mpsc::Sender<EventoTurno>,
    ) -> Result<ManigliaTurno, Errore>;
    /// 2.I: l'account con cui il client è entrato (`None` = non collegato o non leggibile).
    async fn account(&self) -> Result<Option<Account>, Errore> {
        Ok(None)
    }
    /// 2.I: il comando che fa entrare il client (stampa un url), se c'è.
    fn comando_accesso(&self) -> Option<ComandoAccesso> {
        None
    }
    /// 2.I: il comando che fa uscire il client.
    fn comando_uscita(&self) -> Option<ComandoAccesso> {
        None
    }
}
