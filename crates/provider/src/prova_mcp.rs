//! Fase 2.K: la prova di un server MCP (`initialize` + `tools/list`) con `rmcp` come client. Un
//! solo codice per il server (collegamenti senza macchina o della macchina del server) e per il
//! lavoratore (collegamenti di un computer collegato, frame `prova_collegamento`).

use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;

use rmcp::ServiceExt;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, TokioChildProcess};
use serde::{Deserialize, Serialize};
use xbot_core::SegretoTesto;

/// Quanto può durare una prova.
pub const TEMPO_PROVA: Duration = Duration::from_secs(30);

/// Il server da provare, con i segreti già decifrati (solo in memoria).
#[derive(Debug, Clone, Default)]
pub struct ServerDaProvare {
    /// `stdio` | `http`
    pub trasporto: String,
    pub comando: Option<String>,
    pub argomenti: Vec<String>,
    pub ambiente: Vec<(String, SegretoTesto)>,
    pub url: Option<String>,
    pub intestazioni: Vec<(String, SegretoTesto)>,
}

/// Uno strumento visto con `tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StrumentoMcp {
    pub nome: String,
    pub descrizione: Option<String>,
    #[serde(default)]
    pub sola_lettura: bool,
}

/// 2.K: `~/…` diventa `$HOME/…` della macchina dove il comando parte (il catalogo scrive
/// `~/…/Memoria` per la voce della memoria, che vale sul Mac e non sul server). Il resto resta
/// com'è.
pub fn espandi_casa(a: &str) -> String {
    match (a.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(resto), Ok(casa)) if !casa.is_empty() => {
            format!("{}/{resto}", casa.trim_end_matches('/'))
        }
        _ => a.to_string(),
    }
}

fn strumenti(t: Vec<rmcp::model::Tool>) -> Vec<StrumentoMcp> {
    t.into_iter()
        .map(|t| StrumentoMcp {
            sola_lettura: t
                .annotations
                .as_ref()
                .and_then(|a| a.read_only_hint)
                .unwrap_or(false),
            descrizione: t.description.map(|d| d.into_owned()),
            nome: t.name.into_owned(),
        })
        .collect()
}

/// `initialize` + `tools/list`, al massimo per [`TEMPO_PROVA`]. L'errore è un testo per l'utente
/// (mai un segreto: rmcp non stampa le intestazioni).
pub async fn elenco_strumenti(c: &ServerDaProvare) -> Result<Vec<StrumentoMcp>, String> {
    match tokio::time::timeout(TEMPO_PROVA, elenco(c)).await {
        Ok(r) => r,
        Err(_) => Err(format!(
            "nessuna risposta in {} secondi",
            TEMPO_PROVA.as_secs()
        )),
    }
}

async fn elenco(c: &ServerDaProvare) -> Result<Vec<StrumentoMcp>, String> {
    match c.trasporto.as_str() {
        "stdio" => {
            let comando = c
                .comando
                .as_deref()
                .filter(|x| !x.is_empty())
                .ok_or("manca il comando")?;
            let mut cmd = tokio::process::Command::new(espandi_casa(comando));
            cmd.args(c.argomenti.iter().map(|a| espandi_casa(a)))
                .kill_on_drop(true);
            for (k, v) in &c.ambiente {
                cmd.env(k, v.esponi());
            }
            let (trasporto, _) = TokioChildProcess::builder(cmd)
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("{comando} non parte: {e}"))?;
            let client = ().serve(trasporto).await.map_err(|e| format!("initialize fallito: {e}"))?;
            let t = client
                .list_all_tools()
                .await
                .map_err(|e| format!("tools/list fallito: {e}"));
            let _ = client.cancel().await;
            t.map(strumenti)
        }
        "http" => {
            let url = c.url.as_deref().ok_or("manca l'url")?;
            let mut cfg = StreamableHttpClientTransportConfig::with_uri(url.to_string());
            let mut altre = HashMap::new();
            for (k, v) in &c.intestazioni {
                if k.eq_ignore_ascii_case("authorization") {
                    let t = v.esponi();
                    cfg = cfg.auth_header(t.strip_prefix("Bearer ").unwrap_or(t).to_string());
                } else if let (Ok(n), Ok(val)) = (
                    http::HeaderName::from_bytes(k.as_bytes()),
                    http::HeaderValue::from_str(v.esponi()),
                ) {
                    altre.insert(n, val);
                }
            }
            cfg.custom_headers = altre;
            let trasporto = StreamableHttpClientTransport::from_config(cfg);
            let client = ().serve(trasporto).await.map_err(|e| format!("initialize fallito: {e}"))?;
            let t = client
                .list_all_tools()
                .await
                .map_err(|e| format!("tools/list fallito: {e}"));
            let _ = client.cancel().await;
            t.map(strumenti)
        }
        altro => Err(format!("trasporto sconosciuto: {altro}")),
    }
}
