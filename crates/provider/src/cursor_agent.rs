//! L'adattatore `cursor_agent`: lancia `agent` (Cursor Agent, abbonamento Cursor: dentro ci sono
//! Grok, GPT, Claude, Gemini, Composer) con `-p --output-format stream-json
//! --stream-partial-output`, il testo su stdin, e traduce le righe in `EventoTurno`.
//!
//! Formato verificato con `agent` 2026.10.14 (campione registrato a mano):
//! `system/init` (session_id = chatId per `--resume`), `thinking/delta`, `assistant` con
//! `timestamp_ms` (delta) o con `model_call_id` (pezzo completo), `tool_call` `started` /
//! `completed` (esito `result.success` | `result.rejected` | `result.error`), `result` con
//! `usage {inputTokens, outputTokens, cacheReadTokens}`.

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

pub const ID: &str = "cursor_agent";
/// Il modello Grok più economico (usato nelle prove vere).
pub const MODELLO_ECONOMICO: &str = "grok-4.7-low";

#[derive(Debug, Clone)]
pub struct CursorAgent {
    programma: PathBuf,
    sessioni: PathBuf,
    silenzio: Duration,
    attesa_kill: Duration,
}

/// `agent` o `cursor-agent` nel `PATH`.
pub fn trova_nel_path() -> Option<PathBuf> {
    esterni::trova_nel_path(&["agent", "cursor-agent"])
}

impl CursorAgent {
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

/// Gli argomenti di `agent` per una richiesta (il testo va su stdin).
pub fn argomenti(r: &RichiestaTurno) -> Vec<OsString> {
    let mut a: Vec<OsString> = [
        "-p",
        "--output-format",
        "stream-json",
        "--stream-partial-output",
        "--trust",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    if let Some(id) = r.sessione_esterna_id.as_deref().filter(|s| !s.is_empty()) {
        a.push("--resume".into());
        a.push(id.into());
    }
    if let Some(m) = r.modello.as_deref().filter(|s| !s.is_empty()) {
        a.push("--model".into());
        a.push(m.into());
    }
    match Permesso::di(r) {
        Permesso::Mai => {
            a.push("--mode".into());
            a.push("plan".into());
        }
        Permesso::Chiedi => {}
        Permesso::Sempre => a.push("--force".into()),
    }
    // 2.L: i collegamenti del turno stanno in `.cursor/mcp.json` della cartella del turno.
    if !crate::mcp_altri::collegamenti(r).is_empty() {
        a.push("--approve-mcps".into());
    }
    a
}

/// Le righe di `agent --list-models` (`id - etichetta`), senza `auto`.
pub fn leggi_modelli(uscita: &str) -> Vec<Modello> {
    uscita
        .lines()
        .filter_map(|l| l.split_once(" - "))
        .map(|(id, et)| (id.trim(), esterni::pulisci_etichetta(et)))
        .filter(|(id, _)| !id.is_empty() && !id.contains(' ') && *id != "auto")
        .map(|(id, et)| {
            let et = et.trim_end_matches(" (current, default)").to_string();
            Modello {
                id: id.into(),
                etichetta: if et.starts_with("Grok") {
                    format!("{et} (Cursor)")
                } else {
                    et
                },
                dettaglio: id.ends_with("-fast").then(|| "fast".to_string()),
                livelli: Vec::new(),
            }
        })
        .collect()
}

fn modelli_fissi() -> Vec<Modello> {
    leggi_modelli(
        "grok-4.7-low - Grok 4.7 Low\ngrok-4.7-medium - Grok 4.7 Medium\n\
         grok-4.7-high - Grok 4.7 High\ncursor-grok-4.5-high - Grok 4.5\n\
         composer-2.5 - Composer 2.5\ngpt-5.2 - GPT-5.2\n\
         claude-sonnet-5-thinking-high - Claude Sonnet 5 1M Thinking\n\
         gemini-3.7-flash-high - Gemini 3.7 Flash",
    )
}

/// Il nome breve di uno strumento di Cursor: `shellToolCall` → `shell`.
fn nome_strumento(chiave: &str) -> String {
    chiave.trim_end_matches("ToolCall").to_string()
}

/// Lo stato della lettura di un turno di `agent`.
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

    fn assistente(&mut self, v: &Value) -> Vec<EventoTurno> {
        let testo: String = v["message"]["content"]
            .as_array()
            .map(|c| {
                c.iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect()
            })
            .unwrap_or_default();
        if testo.is_empty() {
            return Vec::new();
        }
        let delta = !v["timestamp_ms"].is_null() && v["model_call_id"].is_null();
        if delta {
            self.bolla.push_str(&testo);
            return vec![EventoTurno::TestoDelta(testo)];
        }
        // Un pezzo completo (`model_call_id`) o il riepilogo finale (senza `timestamp_ms`):
        // il testo della bolla dall'ultimo strumento, come deve risultare.
        if self.bolla == testo {
            return Vec::new();
        }
        self.bolla = testo.clone();
        vec![EventoTurno::TestoCompleto(testo)]
    }

    fn strumento(&mut self, v: &Value) -> Vec<EventoTurno> {
        // `tool_call` ha la chiave dello strumento (`shellToolCall`, `readToolCall`…) accanto a
        // `toolCallId`, `startedAtMs`, `hookAdditionalContexts`.
        let Some((chiave, corpo)) = v["tool_call"]
            .as_object()
            .and_then(|o| o.iter().find(|(k, _)| k.ends_with("ToolCall")))
        else {
            return Vec::new();
        };
        let id = v["call_id"].as_str().unwrap_or_default().to_string();
        let nome = nome_strumento(chiave);
        match v["subtype"].as_str() {
            Some("started") => {
                self.bolla.clear();
                self.nomi.insert(id, nome.clone());
                let mut input = corpo["args"].clone();
                // Gli argomenti di Cursor sono lunghi: per la shell basta il comando.
                if let Some(c) = input.get("command").cloned() {
                    input =
                        serde_json::json!({ "command": c, "description": corpo["description"] });
                }
                vec![EventoTurno::ChiamataStrumento {
                    nome,
                    input_json: input,
                }]
            }
            Some("completed") => {
                let nome = self.nomi.remove(&id).unwrap_or(nome);
                let r = &corpo["result"];
                if let Some(rif) = r.get("rejected") {
                    let comando = rif["command"].as_str().unwrap_or_default();
                    return vec![
                        EventoTurno::EsitoStrumento {
                            nome,
                            esito_json: rif.clone(),
                            ok: false,
                        },
                        EventoTurno::Avviso(format!(
                            "Cursor ha negato il comando «{comando}»: senza «sempre» nel permesso \
                             della macchina i comandi che scrivono non partono"
                        )),
                    ];
                }
                let ok = r.get("success").is_some();
                let esito = r
                    .get("success")
                    .or_else(|| r.get("error"))
                    .cloned()
                    .unwrap_or_else(|| r.clone());
                let ok = ok && esito["exitCode"].as_i64().is_none_or(|c| c == 0);
                vec![EventoTurno::EsitoStrumento {
                    nome,
                    esito_json: esito,
                    ok,
                }]
            }
            _ => Vec::new(),
        }
    }

    fn risultato(&mut self, v: &Value) -> Vec<EventoTurno> {
        self.finito = true;
        let sessione = v["session_id"]
            .as_str()
            .map(str::to_string)
            .or_else(|| self.sessione.clone());
        if v["is_error"].as_bool() == Some(true) || v["subtype"] != "success" {
            return vec![EventoTurno::Fallito {
                codice: v["subtype"].as_str().unwrap_or("errore").to_string(),
                testo: v["result"]
                    .as_str()
                    .unwrap_or("Cursor ha chiuso il turno con un errore")
                    .to_string(),
            }];
        }
        let u = &v["usage"];
        vec![EventoTurno::Finito {
            token_ingresso: intero(&u["inputTokens"]),
            token_uscita: intero(&u["outputTokens"]),
            token_cache: intero(&u["cacheReadTokens"]),
            costo_usd: None,
            sessione_esterna_id: sessione,
        }]
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
        if let Some(s) = v["session_id"].as_str() {
            self.sessione = Some(s.to_string());
        }
        match v["type"].as_str().unwrap_or_default() {
            "system" if v["subtype"] == "init" => vec![EventoTurno::Avviato {
                sessione_esterna_id: self.sessione.clone(),
            }],
            "thinking" if v["subtype"] == "delta" => v["text"]
                .as_str()
                .filter(|t| !t.is_empty())
                .map(|t| vec![EventoTurno::Ragionamento(t.to_string())])
                .unwrap_or_default(),
            "assistant" => self.assistente(&v),
            "tool_call" => self.strumento(&v),
            "result" => self.risultato(&v),
            _ => Vec::new(),
        }
    }

    fn finito(&self) -> bool {
        self.finito
    }
}

#[async_trait]
impl Provider for CursorAgent {
    fn id(&self) -> &str {
        ID
    }

    fn nome(&self) -> &str {
        "Cursor"
    }

    fn programma(&self) -> &str {
        self.programma.to_str().unwrap_or("agent")
    }

    /// Da `agent --list-models` (20 s al massimo); se non risponde, un elenco fisso.
    async fn modelli(&self) -> Result<Vec<Modello>, Errore> {
        let mut cmd = tokio::process::Command::new(&self.programma);
        cmd.arg("--list-models")
            .env("NO_COLOR", "1")
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        let uscita = tokio::time::timeout(Duration::from_secs(20), cmd.output()).await;
        let m = match uscita {
            Ok(Ok(o)) if o.status.success() => leggi_modelli(&String::from_utf8_lossy(&o.stdout)),
            _ => Vec::new(),
        };
        // Tutti, anche le varianti «fast» (2026-10-25 05:40: «devo poter cambiare i modelli
        // come e quando voglio»).
        Ok(if m.is_empty() { modelli_fissi() } else { m })
    }

    async fn account(&self) -> Result<Option<crate::Account>, Errore> {
        let (_, t) =
            crate::accesso::uscita(&self.programma, &["status", "--format", "json"]).await?;
        Ok(crate::accesso::leggi_cursor(&t))
    }

    fn comando_accesso(&self) -> Option<crate::ComandoAccesso> {
        let mut c = crate::ComandoAccesso::nuovo(&self.programma, &["login"]);
        c.ambiente.push(("NO_OPEN_BROWSER".into(), "1".into()));
        Some(c)
    }

    fn comando_uscita(&self) -> Option<crate::ComandoAccesso> {
        Some(crate::ComandoAccesso::nuovo(&self.programma, &["logout"]))
    }

    async fn quota(&self) -> Result<Option<Quota>, Errore> {
        Ok(None)
    }

    async fn avvia(
        &self,
        richiesta: RichiestaTurno,
        eventi: mpsc::Sender<EventoTurno>,
    ) -> Result<ManigliaTurno, Errore> {
        let cartella = self.cartella_di(&richiesta);
        // 2.L-ter: `agent` legge `.cursor/mcp.json` dalla radice git, non dalla cartella.
        let propria = richiesta.cartella_lavoro.is_none()
            && !crate::mcp_altri::collegamenti(&richiesta).is_empty();
        let radice = crate::mcp_altri::radice_cursor(&cartella, propria).await;
        if !crate::mcp_altri::scrivi_cursor(&radice, &richiesta)? {
            let t = "la cartella ha già un .cursor/mcp.json dell'utente: i collegamenti di XBot non entrano in questo turno";
            let _ = eventi.send(EventoTurno::Avviso(t.into())).await;
        }
        let lancio = Lancio {
            nome: "Cursor (agent)",
            programma: self.programma.clone(),
            argomenti: argomenti(&richiesta),
            cartella,
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
#[path = "cursor_agent_prove.rs"]
mod prove;
