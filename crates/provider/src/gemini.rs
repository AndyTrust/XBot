//! L'adattatore `gemini`: lancia `gemini --prompt=<testo> -o stream-json` e traduce le righe.
//!
//! Formato letto dal sorgente di gemini-cli 0.31.0 (`nonInteractiveCli.js`,
//! `output/types.js`), perché sul Mac dell'utente il client non entra più
//! (`IneligibleTierError: UNSUPPORTED_CLIENT`, 2026-10-24): `init {session_id, model}`,
//! `message {role, content, delta}`, `tool_use {tool_name, tool_id, parameters}`,
//! `tool_result {tool_id, status, output, error}`, `error {severity, message}`,
//! `result {status, error?, stats {input_tokens, output_tokens, cached}}`.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::mpsc;
use xbot_core::Errore;

use crate::esterni::{self, Lancio, Permesso, Traduttore, intero};
use crate::{EventoTurno, ManigliaTurno, Modello, Provider, Quota, RichiestaTurno};

pub const ID: &str = "gemini";
pub const MODELLO_ECONOMICO: &str = "gemini-2.5-flash-lite";

#[derive(Debug, Clone)]
pub struct Gemini {
    programma: PathBuf,
    sessioni: PathBuf,
    silenzio: Duration,
    attesa_kill: Duration,
}

pub fn trova_nel_path() -> Option<PathBuf> {
    esterni::trova_nel_path(&["gemini"])
}

impl Gemini {
    pub fn nuovo(programma: impl Into<PathBuf>) -> Self {
        Self {
            programma: programma.into(),
            sessioni: crate::claude_code::cartella_sessioni_predefinita(),
            silenzio: Duration::from_secs(600),
            attesa_kill: Duration::from_secs(5),
        }
    }

    pub fn con_cartella_sessioni(mut self, cartella: impl Into<PathBuf>) -> Self {
        self.sessioni = cartella.into();
        self
    }

    pub fn con_silenzio(mut self, silenzio: Duration) -> Self {
        self.silenzio = silenzio;
        self
    }

    pub fn cartella_di(&self, r: &RichiestaTurno) -> PathBuf {
        esterni::cartella_di(&self.sessioni, r)
    }
}

/// Gli argomenti di `gemini`. Il testo va in `--prompt=…` (una sola parola: un testo che
/// comincia con `-` non diventa un'opzione). `plan` in 0.31.0 vuole `experimental.plan`,
/// altrimenti ripiega su `default`: per «mai» si passa `default`, che in headless nega gli
/// strumenti da approvare.
pub fn argomenti(r: &RichiestaTurno) -> Vec<OsString> {
    let mut a: Vec<OsString> = vec!["-o".into(), "stream-json".into()];
    if let Some(id) = r.sessione_esterna_id.as_deref().filter(|s| !s.is_empty()) {
        a.push("-r".into());
        a.push(id.into());
    }
    if let Some(m) = r.modello.as_deref().filter(|s| !s.is_empty()) {
        a.push("-m".into());
        a.push(m.into());
    }
    let modo = match Permesso::di(r) {
        Permesso::Mai => "default",
        Permesso::Chiedi => "auto_edit",
        Permesso::Sempre => "yolo",
    };
    a.push(format!("--approval-mode={modo}").into());
    a.push(format!("--prompt={}", esterni::testo_del_turno(r)).into());
    a
}

fn modello(id: &str, etichetta: &str, dettaglio: &str) -> Modello {
    Modello {
        id: id.into(),
        etichetta: etichetta.into(),
        dettaglio: Some(dettaglio.into()),
        livelli: Vec::new(),
    }
}

/// Lo stato della lettura di un turno di `gemini`.
#[derive(Debug, Default)]
pub struct Lettore {
    bolla: String,
    nomi: HashMap<String, String>,
    sessione: Option<String>,
    finito: bool,
}

impl Lettore {
    pub fn nuovo() -> Self {
        Self::default()
    }

    pub fn sessione(&self) -> Option<&str> {
        self.sessione.as_deref()
    }
}

impl Traduttore for Lettore {
    fn riga(&mut self, linea: &str) -> Vec<EventoTurno> {
        let linea = linea.trim();
        if linea.is_empty() {
            return Vec::new();
        }
        let Ok(v) = serde_json::from_str::<Value>(linea) else {
            tracing::debug!(lunghezza = linea.len(), "riga non JSON ignorata");
            return Vec::new();
        };
        match v["type"].as_str().unwrap_or_default() {
            "init" => {
                self.sessione = v["session_id"].as_str().map(str::to_string);
                vec![EventoTurno::Avviato {
                    sessione_esterna_id: self.sessione.clone(),
                }]
            }
            "message" if v["role"] == "assistant" => {
                let t = v["content"].as_str().unwrap_or_default().to_string();
                if t.is_empty() {
                    return Vec::new();
                }
                if v["delta"].as_bool() == Some(true) {
                    self.bolla.push_str(&t);
                    vec![EventoTurno::TestoDelta(t)]
                } else {
                    self.bolla = t.clone();
                    vec![EventoTurno::TestoCompleto(t)]
                }
            }
            "tool_use" => {
                self.bolla.clear();
                let nome = v["tool_name"].as_str().unwrap_or("strumento").to_string();
                if let Some(id) = v["tool_id"].as_str() {
                    self.nomi.insert(id.to_string(), nome.clone());
                }
                vec![EventoTurno::ChiamataStrumento {
                    nome,
                    input_json: v["parameters"].clone(),
                }]
            }
            "tool_result" => {
                let nome = v["tool_id"]
                    .as_str()
                    .and_then(|id| self.nomi.remove(id))
                    .unwrap_or_else(|| "strumento".into());
                let ok = v["status"] == "success";
                let mut ev = vec![EventoTurno::EsitoStrumento {
                    nome,
                    esito_json: serde_json::json!({ "output": v["output"], "error": v["error"] }),
                    ok,
                }];
                if !ok {
                    let msg = v["error"]["message"].as_str().unwrap_or_default();
                    if msg.contains("approv")
                        || msg.contains("denied")
                        || msg.contains("not allowed")
                    {
                        ev.push(EventoTurno::Avviso(format!(
                            "Gemini ha negato lo strumento: {msg}"
                        )));
                    }
                }
                ev
            }
            "error" => vec![EventoTurno::Avviso(
                v["message"]
                    .as_str()
                    .unwrap_or("avviso di gemini")
                    .to_string(),
            )],
            "result" => {
                self.finito = true;
                if v["status"] != "success" {
                    return vec![EventoTurno::Fallito {
                        codice: v["error"]["type"].as_str().unwrap_or("errore").to_string(),
                        testo: v["error"]["message"]
                            .as_str()
                            .unwrap_or("gemini ha chiuso il turno con un errore")
                            .to_string(),
                    }];
                }
                let s = &v["stats"];
                vec![EventoTurno::Finito {
                    token_ingresso: intero(&s["input_tokens"]),
                    token_uscita: intero(&s["output_tokens"]),
                    token_cache: intero(&s["cached"]),
                    costo_usd: None,
                    sessione_esterna_id: self.sessione.clone(),
                }]
            }
            _ => Vec::new(),
        }
    }

    fn finito(&self) -> bool {
        self.finito
    }
}

#[async_trait]
impl Provider for Gemini {
    fn id(&self) -> &str {
        ID
    }

    fn nome(&self) -> &str {
        "Gemini"
    }

    fn programma(&self) -> &str {
        self.programma.to_str().unwrap_or("gemini")
    }

    /// I modelli che gemini-cli 0.31.0 conosce (da `config/models.js`).
    async fn modelli(&self) -> Result<Vec<Modello>, Errore> {
        Ok(vec![
            modello("gemini-3.1-pro-preview", "Gemini 3.1 Pro", "il più capace"),
            modello("gemini-3-flash-preview", "Gemini 3 Flash", "veloce"),
            modello("gemini-2.5-pro", "Gemini 2.5 Pro", "capace"),
            modello("gemini-2.5-flash", "Gemini 2.5 Flash", "equilibrato"),
            modello(
                MODELLO_ECONOMICO,
                "Gemini 2.5 Flash Lite",
                "il più economico",
            ),
        ])
    }

    async fn quota(&self) -> Result<Option<Quota>, Errore> {
        Ok(None)
    }

    async fn avvia(
        &self,
        richiesta: RichiestaTurno,
        eventi: mpsc::Sender<EventoTurno>,
    ) -> Result<ManigliaTurno, Errore> {
        let lancio = Lancio {
            nome: "gemini",
            programma: self.programma.clone(),
            argomenti: argomenti(&richiesta),
            cartella: self.cartella_di(&richiesta),
            ambiente: richiesta.ambiente.clone(),
            ambiente_adattatore: Vec::new(),
            stdin: None,
            silenzio: self.silenzio,
            attesa_kill: self.attesa_kill,
        };
        esterni::avvia(lancio, Lettore::nuovo(), eventi).await
    }
}

#[cfg(test)]
#[path = "gemini_prove.rs"]
mod prove;
