//! Fase 2.L-bis: grok in ACP risponde a `session/new` (o `session/load`) prima che i server MCP
//! della casa abbiano finito l'`initialize`, e il primo `session/prompt` parte con gli strumenti
//! di allora (`tool_prep_done … mcp_wait_ms: 0`, «No MCP tools are available»). Grok 1.0.70 lo
//! dice con notifiche sue (lette il 2026-10-25 con un server finto lento 2 s):
//! `_x.ai/mcp/servers_updated {mcpServers: [...]}` durante `session/new`, poi
//! `_x.ai/mcp/init_progress {total, connected}`, `_x.ai/mcp/server_status {name, status}` e
//! `_x.ai/mcp_initialized {mcpToolCount, elapsedMs}`. Qui si contano e il processo aspetta, prima
//! del primo turno, che siano pronti (al massimo [`TETTO`], come `startup_timeout_sec`).

use std::collections::HashSet;
use std::time::Duration;

use serde_json::{Value, json};

use super::{Processo, Stop};
use crate::EventoTurno;

/// Quanto si aspetta al più (lo `startup_timeout_sec` che XBot scrive nella casa).
pub const TETTO: Duration = Duration::from_secs(60);

/// Lo stato dei server MCP della sessione, dalle notifiche di grok.
#[derive(Debug, Default)]
pub struct Pronti {
    /// I server dichiarati (`servers_updated`); `None` = nessuna notizia (nessuna attesa).
    attesi: Option<usize>,
    /// I server che hanno finito, bene o male (`server_status`).
    finiti: HashSet<String>,
    /// `mcp_initialized` o `init_progress` con `connected == total`.
    fatto: bool,
}

impl Pronti {
    /// Una notifica di grok (le altre si ignorano).
    pub fn vedi(&mut self, metodo: &str, p: &Value) {
        match metodo {
            "_x.ai/mcp/servers_updated" => {
                let n = p["mcpServers"].as_array().map_or(0, Vec::len);
                self.attesi = Some(n);
            }
            "_x.ai/mcp/init_progress" => {
                if p["total"]
                    .as_u64()
                    .is_some_and(|t| p["connected"].as_u64() >= Some(t))
                {
                    self.fatto = true;
                }
            }
            "_x.ai/mcp/server_status" => {
                let s = p["status"].as_str().unwrap_or_default();
                if s != "pending"
                    && s != "connecting"
                    && let Some(n) = p["name"].as_str()
                {
                    self.finiti.insert(n.to_string());
                }
            }
            "_x.ai/mcp_initialized" => self.fatto = true,
            _ => {}
        }
    }

    /// Vero se non c'è niente da aspettare.
    pub fn pronti(&self) -> bool {
        match self.attesi {
            None | Some(0) => true,
            Some(n) => self.fatto || self.finiti.len() >= n,
        }
    }
}

impl Processo {
    /// Legge le righe di grok finché i server MCP sono pronti o passa il [`TETTO`]. Una richiesta
    /// di grok in questo tempo riceve -32601 (come in `chiama` senza turno).
    pub(super) async fn aspetta_mcp(
        &mut self,
        eventi: &tokio::sync::mpsc::Sender<EventoTurno>,
    ) -> Result<(), Stop> {
        let fine = tokio::time::Instant::now() + TETTO;
        while !self.pronti.pronti() {
            let riga = match tokio::time::timeout_at(fine, self.righe.next_line()).await {
                Err(_) => {
                    let t = "i collegamenti non hanno risposto in 60 secondi: il turno parte senza i loro strumenti";
                    let _ = eventi.send(EventoTurno::Avviso(t.into())).await;
                    return Ok(());
                }
                Ok(Err(e)) => return Err(Stop::Uscito(format!("lettura di stdout: {e}"))),
                Ok(Ok(None)) => return Err(Stop::Uscito(String::new())),
                Ok(Ok(Some(l))) => l,
            };
            let Ok(v) = serde_json::from_str::<Value>(riga.trim()) else {
                continue;
            };
            let metodo = v["method"].as_str().unwrap_or_default();
            match v.get("id").filter(|i| !i.is_null()) {
                Some(rid) if !metodo.is_empty() => {
                    let r = json!({ "jsonrpc": "2.0", "id": rid, "error": {
                        "code": -32601, "message": "metodo non gestito dal client XBot" } });
                    self.scrivi(&r).await.map_err(Stop::Uscito)?;
                }
                _ => self.pronti.vedi(metodo, &v["params"]),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn notifiche_vere_di_grok() {
        let mut p = Pronti::default();
        assert!(p.pronti(), "senza notizie non si aspetta");
        p.vedi(
            "_x.ai/mcp/servers_updated",
            &json!({ "mcpServers": [{ "name": "lento" }] }),
        );
        assert!(!p.pronti());
        p.vedi(
            "_x.ai/mcp/init_progress",
            &json!({ "total": 1, "connected": 0 }),
        );
        assert!(!p.pronti());
        p.vedi(
            "_x.ai/mcp_initialized",
            &json!({ "mcpToolCount": 1, "elapsedMs": 2024 }),
        );
        assert!(p.pronti());
        let mut p = Pronti::default();
        p.vedi(
            "_x.ai/mcp/servers_updated",
            &json!({ "mcpServers": [{}, {}] }),
        );
        p.vedi(
            "_x.ai/mcp/server_status",
            &json!({ "name": "a", "status": "failed" }),
        );
        assert!(!p.pronti());
        p.vedi(
            "_x.ai/mcp/server_status",
            &json!({ "name": "b", "status": "ready" }),
        );
        assert!(p.pronti(), "anche un server rotto chiude l'attesa");
        let mut p = Pronti::default();
        p.vedi("_x.ai/mcp/servers_updated", &json!({ "mcpServers": [] }));
        assert!(p.pronti());
    }
}
