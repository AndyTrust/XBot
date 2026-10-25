//! L'adattatore `codex`: lancia `codex exec --json` (abbonamento ChatGPT), il testo su stdin
//! (`-`), e traduce le righe JSONL in `EventoTurno`.
//!
//! Formato verificato con `codex-cli` 0.157.1 (campione registrato a mano): `thread.started`
//! (`thread_id` per `codex exec resume <id>`), `turn.started`, `item.started` /
//! `item.completed` con `item.type` = `agent_message` | `reasoning` | `command_execution` |
//! `file_change` | `mcp_tool_call` | `web_search` | `error`, `turn.completed` con `usage`,
//! `turn.failed` / `error`. Il testo arriva a messaggi interi, non a pezzi.

use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::mpsc;
use xbot_core::Errore;

use crate::esterni::{self, Lancio, Permesso, Traduttore, intero};
use crate::{EventoTurno, ManigliaTurno, Modello, Provider, Quota, RichiestaTurno};

pub const ID: &str = "codex";
pub const MODELLO_ECONOMICO: &str = "gpt-5.6-luna";
/// I livelli quando il catalogo non si legge (codex-cli 0.157.1: i modelli `sol`, `terra` e `astra`
/// hanno anche `ultra`).
const LIVELLI: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

#[derive(Debug, Clone)]
pub struct Codex {
    programma: PathBuf,
    sessioni: PathBuf,
    silenzio: Duration,
    attesa_kill: Duration,
}

pub fn trova_nel_path() -> Option<PathBuf> {
    esterni::trova_nel_path(&["codex"])
}

impl Codex {
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

/// Gli argomenti di `codex` (il testo arriva da stdin con `-`). `resume` non accetta `-s` né
/// `-C`: la sandbox passa come `-c sandbox_mode=…`, la cartella è quella del processo.
pub fn argomenti(r: &RichiestaTurno) -> Vec<OsString> {
    let mut a: Vec<OsString> = vec!["exec".into()];
    let ripresa = r.sessione_esterna_id.as_deref().filter(|s| !s.is_empty());
    if ripresa.is_some() {
        a.push("resume".into());
    }
    for x in ["--json", "--skip-git-repo-check"] {
        a.push(x.into());
    }
    if let Some(m) = r.modello.as_deref().filter(|s| !s.is_empty()) {
        a.push("-m".into());
        a.push(m.into());
    }
    if let Some(l) = r.livello.as_deref().filter(|l| livello_valido(l)) {
        a.push("-c".into());
        a.push(format!("model_reasoning_effort=\"{l}\"").into());
    }
    let sandbox = match Permesso::di(r) {
        Permesso::Mai => "read-only",
        // Anche con «sempre» la sandbox resta: mai `--dangerously-bypass-approvals-and-sandbox`.
        Permesso::Chiedi | Permesso::Sempre => "workspace-write",
    };
    a.push("-c".into());
    a.push(format!("sandbox_mode=\"{sandbox}\"").into());
    a.push("-c".into());
    a.push("approval_policy=\"never\"".into());
    if let Some(id) = ripresa {
        a.push(id.into());
    }
    a.push("-".into());
    a
}

/// Un livello accettabile: una parola semplice (il catalogo dice quali vanno per ogni modello).
fn livello_valido(l: &str) -> bool {
    !l.is_empty()
        && l.len() <= 32
        && l.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// I modelli dal catalogo di codex (`codex debug models` o `~/.codex/models_cache.json`, stessa
/// forma `{models: [{slug, display_name, description, visibility, supported_reasoning_levels}]}`).
/// Tutti, anche quelli che codex nasconde (dettaglio «nascosto in codex»): sceglie l'utente.
pub fn leggi_catalogo(json: &str) -> Vec<Modello> {
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let Some(ms) = v["models"].as_array() else {
        return Vec::new();
    };
    let mut visibili = Vec::new();
    let mut nascosti = Vec::new();
    for m in ms {
        let Some(id) = m["slug"].as_str().filter(|s| !s.is_empty()) else {
            continue;
        };
        let livelli: Vec<String> = m["supported_reasoning_levels"]
            .as_array()
            .map(|l| {
                l.iter()
                    .filter_map(|x| x["effort"].as_str())
                    .filter(|x| livello_valido(x))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let nascosto = m["visibility"] == "hide";
        let descrizione = m["description"].as_str().unwrap_or_default().trim();
        let dettaglio = match (nascosto, descrizione.is_empty()) {
            (true, _) => Some("nascosto in codex".to_string()),
            (false, false) => Some(descrizione.to_string()),
            _ => None,
        };
        let modello = Modello {
            id: id.into(),
            etichetta: m["display_name"].as_str().unwrap_or(id).to_string(),
            dettaglio,
            livelli,
        };
        if nascosto {
            nascosti.push(modello);
        } else {
            visibili.push(modello);
        }
    }
    visibili.extend(nascosti);
    visibili
}

/// L'elenco intero quando il catalogo non si legge.
fn modelli_fissi() -> Vec<Modello> {
    vec![
        modello("gpt-6.1-sol", "GPT-6.1 Sol", "il più capace"),
        modello("gpt-6-sol", "GPT-6 Sol", "capace"),
        modello("gpt-6-astra", "GPT-6 Astra", "equilibrato"),
        modello("gpt-6-luna", "GPT-6 Luna", "veloce"),
        modello("gpt-5.6-sol", "GPT-5.6 Sol", "generazione prima"),
        modello(
            "gpt-5.6-terra",
            "GPT-5.6 Terra",
            "generazione prima, equilibrato",
        ),
        modello("gpt-5.6-luna", "GPT-5.6 Luna", "generazione prima, veloce"),
    ]
}

fn modello(id: &str, etichetta: &str, dettaglio: &str) -> Modello {
    Modello {
        id: id.into(),
        etichetta: etichetta.into(),
        dettaglio: Some(dettaglio.into()),
        livelli: LIVELLI.iter().map(|l| l.to_string()).collect(),
    }
}

/// Lo stato della lettura di un turno di `codex`.
#[derive(Debug, Default)]
pub struct Lettore {
    bolla: String,
    sessione: Option<String>,
    finito: bool,
}

fn nome_item(item: &Value) -> String {
    match item["type"].as_str().unwrap_or_default() {
        "command_execution" => "shell".into(),
        "mcp_tool_call" => format!(
            "mcp__{}__{}",
            item["server"].as_str().unwrap_or("?"),
            item["tool"].as_str().unwrap_or("?")
        ),
        altro => altro.to_string(),
    }
}

/// Le chiavi dell'item che descrivono la chiamata (senza id, stato ed esito).
fn input_item(item: &Value) -> Value {
    match item["type"].as_str().unwrap_or_default() {
        "command_execution" => serde_json::json!({ "command": item["command"] }),
        "mcp_tool_call" => item["arguments"].clone(),
        "file_change" => item["changes"].clone(),
        "web_search" => serde_json::json!({ "query": item["query"] }),
        _ => Value::Null,
    }
}

impl Lettore {
    pub fn nuovo() -> Self {
        Self::default()
    }

    pub fn sessione(&self) -> Option<&str> {
        self.sessione.as_deref()
    }

    fn item(&mut self, inizio: bool, item: &Value) -> Vec<EventoTurno> {
        let tipo = item["type"].as_str().unwrap_or_default();
        match (tipo, inizio) {
            ("agent_message", false) => {
                let t = item["text"].as_str().unwrap_or_default();
                if t.is_empty() {
                    return Vec::new();
                }
                if !self.bolla.is_empty() {
                    self.bolla.push_str("\n\n");
                }
                self.bolla.push_str(t);
                vec![EventoTurno::TestoCompleto(self.bolla.clone())]
            }
            ("reasoning", false) => item["text"]
                .as_str()
                .map(|t| vec![EventoTurno::Ragionamento(t.to_string())])
                .unwrap_or_default(),
            ("error", false) => vec![EventoTurno::Avviso(
                item["message"].as_str().unwrap_or("avviso di codex").into(),
            )],
            ("command_execution" | "mcp_tool_call" | "file_change" | "web_search", true) => {
                self.bolla.clear();
                vec![EventoTurno::ChiamataStrumento {
                    nome: nome_item(item),
                    input_json: input_item(item),
                }]
            }
            ("command_execution" | "mcp_tool_call" | "file_change" | "web_search", false) => {
                let stato = item["status"].as_str().unwrap_or_default();
                let ok = stato != "failed"
                    && stato != "declined"
                    && item["exit_code"].as_i64().is_none_or(|c| c == 0)
                    && item["error"].is_null();
                let esito = match tipo {
                    "command_execution" => serde_json::json!({
                        "output": item["aggregated_output"],
                        "exit_code": item["exit_code"],
                        "status": item["status"],
                    }),
                    "mcp_tool_call" => serde_json::json!({
                        "result": item["result"], "error": item["error"], "status": item["status"],
                    }),
                    _ => item.clone(),
                };
                let mut v = Vec::new();
                // Un comando che non ha mai avuto `item.started` (rifiutato prima) apre lo strumento qui.
                if item["status"] == "declined" {
                    v.push(EventoTurno::Avviso(
                        "codex ha rifiutato il comando: la sandbox della macchina non lo permette"
                            .into(),
                    ));
                }
                v.insert(
                    0,
                    EventoTurno::EsitoStrumento {
                        nome: nome_item(item),
                        esito_json: esito,
                        ok,
                    },
                );
                v
            }
            _ => Vec::new(),
        }
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
            "thread.started" => {
                self.sessione = v["thread_id"].as_str().map(str::to_string);
                vec![EventoTurno::Avviato {
                    sessione_esterna_id: self.sessione.clone(),
                }]
            }
            "item.started" => self.item(true, &v["item"]),
            "item.completed" => self.item(false, &v["item"]),
            "turn.completed" => {
                self.finito = true;
                let u = &v["usage"];
                // --- 2.I --- la quota la scrive codex nel file della sessione.
                let mut e = self
                    .sessione
                    .as_deref()
                    .map(crate::codex_quota::quote)
                    .unwrap_or_default();
                e.push(EventoTurno::Finito {
                    token_ingresso: intero(&u["input_tokens"]),
                    token_uscita: intero(&u["output_tokens"]),
                    token_cache: intero(&u["cached_input_tokens"]),
                    costo_usd: None,
                    sessione_esterna_id: self.sessione.clone(),
                });
                e
            }
            "turn.failed" => {
                self.finito = true;
                vec![EventoTurno::Fallito {
                    codice: "turno_fallito".into(),
                    testo: v["error"]["message"]
                        .as_str()
                        .unwrap_or("codex ha chiuso il turno con un errore")
                        .to_string(),
                }]
            }
            // Un errore di flusso (riconnessione, limite): resta un avviso, chiude `turn.failed`.
            "error" => vec![EventoTurno::Avviso(
                v["message"]
                    .as_str()
                    .unwrap_or("errore di codex")
                    .to_string(),
            )],
            _ => Vec::new(),
        }
    }

    fn finito(&self) -> bool {
        self.finito
    }
}

#[async_trait]
impl Provider for Codex {
    fn id(&self) -> &str {
        ID
    }

    fn nome(&self) -> &str {
        "Codex / ChatGPT"
    }

    fn programma(&self) -> &str {
        self.programma.to_str().unwrap_or("codex")
    }

    /// Tutti i modelli del catalogo di codex (`codex debug models`, 20 s al massimo), poi
    /// `~/.codex/models_cache.json`, poi l'elenco fisso.
    async fn modelli(&self) -> Result<Vec<Modello>, Errore> {
        let mut cmd = tokio::process::Command::new(&self.programma);
        cmd.args(["debug", "models"])
            .env("NO_COLOR", "1")
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        let m = match tokio::time::timeout(Duration::from_secs(20), cmd.output()).await {
            Ok(Ok(o)) if o.status.success() => leggi_catalogo(&String::from_utf8_lossy(&o.stdout)),
            _ => Vec::new(),
        };
        if !m.is_empty() {
            return Ok(m);
        }
        let cache = std::env::var_os("CODEX_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex")))
            .map(|d| d.join("models_cache.json"));
        let m = match cache {
            Some(p) => tokio::fs::read_to_string(p)
                .await
                .map(|t| leggi_catalogo(&t))
                .unwrap_or_default(),
            None => Vec::new(),
        };
        Ok(if m.is_empty() { modelli_fissi() } else { m })
    }

    async fn account(&self) -> Result<Option<crate::Account>, Errore> {
        let (_, t) = crate::accesso::uscita(&self.programma, &["login", "status"]).await?;
        Ok(crate::accesso::leggi_codex(&t))
    }

    fn comando_accesso(&self) -> Option<crate::ComandoAccesso> {
        Some(crate::ComandoAccesso::nuovo(
            &self.programma,
            &["login", "--device-auth"],
        ))
    }

    fn comando_uscita(&self) -> Option<crate::ComandoAccesso> {
        Some(crate::ComandoAccesso::nuovo(&self.programma, &["logout"]))
    }

    /// La quota arriva con i turni (`codex_quota`), non da un comando.
    async fn quota(&self) -> Result<Option<Quota>, Errore> {
        Ok(None)
    }

    async fn avvia(
        &self,
        richiesta: RichiestaTurno,
        eventi: mpsc::Sender<EventoTurno>,
    ) -> Result<ManigliaTurno, Errore> {
        let lancio = Lancio {
            nome: "codex",
            programma: self.programma.clone(),
            argomenti: argomenti(&richiesta),
            cartella: self.cartella_di(&richiesta),
            ambiente: richiesta.ambiente.clone(),
            ambiente_adattatore: Vec::new(),
            stdin: Some(esterni::testo_del_turno(&richiesta)),
            silenzio: self.silenzio,
            attesa_kill: self.attesa_kill,
        };
        esterni::avvia(lancio, Lettore::nuovo(), eventi).await
    }
}

#[cfg(test)]
#[path = "codex_prove.rs"]
mod prove;
