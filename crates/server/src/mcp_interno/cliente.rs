//! Fase 2.G: chi decide i permessi ACP di grok sul server. Si collega al socket del server MCP
//! interno come farebbe il ponte di Claude Code e chiama lo strumento `permesso`: così le
//! richieste di grok passano per la stessa strada (concessioni, regole, domanda all'utente).

use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use xbot_provider::ChiediPermesso;

/// Un client MCP minimo verso il socket, per una sessione.
#[derive(Debug, Clone)]
pub struct PermessoDalSocket {
    pub socket: PathBuf,
    pub sessione_id: String,
    pub gettone: String,
}

impl PermessoDalSocket {
    async fn chiama(&self, strumento: &str, input: &Value) -> Result<Value, String> {
        let flusso = UnixStream::connect(&self.socket)
            .await
            .map_err(|e| format!("socket dei permessi: {e}"))?;
        let (l, mut s) = flusso.into_split();
        let mut l = BufReader::new(l);
        let righe = [
            json!({ "sessione": self.sessione_id, "gettone": self.gettone }),
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": { "name": "xbot-grok", "version": xbot_core::VERSIONE } } }),
        ];
        for r in &righe {
            s.write_all(format!("{r}\n").as_bytes())
                .await
                .map_err(|e| e.to_string())?;
        }
        leggi(&mut l, 1).await?;
        let chiamata = [
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
                "name": "permesso", "arguments": { "tool_name": strumento, "input": input } } }),
        ];
        for r in &chiamata {
            s.write_all(format!("{r}\n").as_bytes())
                .await
                .map_err(|e| e.to_string())?;
        }
        let r = leggi(&mut l, 2).await?;
        let testo = r["content"][0]["text"].as_str().unwrap_or_default();
        serde_json::from_str(testo).map_err(|e| format!("risposta del permesso illeggibile: {e}"))
    }
}

async fn leggi(
    l: &mut BufReader<tokio::net::unix::OwnedReadHalf>,
    id: u64,
) -> Result<Value, String> {
    loop {
        let mut riga = String::new();
        if l.read_line(&mut riga).await.map_err(|e| e.to_string())? == 0 {
            return Err("il server dei permessi ha chiuso".into());
        }
        let Ok(v) = serde_json::from_str::<Value>(riga.trim()) else {
            continue;
        };
        if v["id"].as_u64() == Some(id) {
            if let Some(e) = v.get("error") {
                return Err(e.to_string());
            }
            return Ok(v["result"].clone());
        }
    }
}

#[async_trait]
impl ChiediPermesso for PermessoDalSocket {
    async fn chiedi(&self, strumento: &str, input: &Value) -> Result<(), String> {
        let v = self.chiama(strumento, input).await?;
        if v["behavior"] == "allow" {
            Ok(())
        } else {
            Err(v["message"].as_str().unwrap_or("negato").to_string())
        }
    }
}
