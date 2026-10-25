//! La scoperta dei client nel `PATH` del Mac e dei loro modelli.
//!
//! I modelli li dà `Provider::modelli()` di ogni adattatore: `claude` i tre fissi del server,
//! `agent`/`cursor-agent` da `agent --list-models`, `codex` da `codex debug models`,
//! `gemini` l'elenco di S-A, `grok` da `grok models` (livelli dalla sonda di `--reasoning-effort`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use xbot_provider::{
    ClaudeCode, Codex, CursorAgent, Gemini, Grok, Provider, codex, cursor_agent, gemini, grok,
};

use crate::protocollo::{ModelloMacchina, ProviderMacchina};

/// Cerca un programma nel `PATH` (senza shell: in zsh un nome può essere una funzione).
pub fn nel_path(nome: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(nome))
        .find(|p| p.is_file())
}

async fn uscita_di(programma: &Path, argomenti: &[&str], attesa: Duration) -> Option<String> {
    let mut c = tokio::process::Command::new(programma);
    c.args(argomenti)
        .stdin(std::process::Stdio::null())
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .kill_on_drop(true);
    match tokio::time::timeout(attesa, c.output()).await {
        Ok(Ok(o)) if o.status.success() => Some(String::from_utf8_lossy(&o.stdout).into_owned()),
        Ok(Ok(o)) => {
            tracing::warn!(programma = %programma.display(), stato = %o.status, "uscita non riuscita");
            None
        }
        Ok(Err(e)) => {
            tracing::warn!(programma = %programma.display(), errore = %e, "non parte");
            None
        }
        Err(_) => {
            tracing::warn!(programma = %programma.display(), "troppo lento: saltato");
            None
        }
    }
}

async fn versione(programma: &Path) -> Option<String> {
    uscita_di(programma, &["--version"], Duration::from_secs(20))
        .await
        .map(|v| v.lines().next().unwrap_or_default().trim().to_string())
        .filter(|v| !v.is_empty())
}

type Crea = fn(&Path) -> Box<dyn Provider>;

/// 2.I: l'account del client (30 s al massimo; un errore vale «non collegato»).
pub async fn leggi_account(a: &dyn Provider) -> Option<xbot_provider::Account> {
    match tokio::time::timeout(Duration::from_secs(30), a.account()).await {
        Ok(Ok(x)) => x,
        Ok(Err(e)) => {
            tracing::warn!(provider = a.id(), errore = %e, "account non letto");
            None
        }
        Err(_) => None,
    }
}

/// I client trovati sul Mac con i loro modelli (dagli adattatori di `xbot-provider`).
pub async fn scopri() -> Vec<ProviderMacchina> {
    let mut trovati = Vec::new();
    let candidati: [(Option<PathBuf>, Crea); 5] = [
        (nel_path("claude"), |p| Box::new(ClaudeCode::nuovo(p))),
        (cursor_agent::trova_nel_path(), |p| {
            Box::new(CursorAgent::nuovo(p))
        }),
        (codex::trova_nel_path(), |p| Box::new(Codex::nuovo(p))),
        (gemini::trova_nel_path(), |p| Box::new(Gemini::nuovo(p))),
        (grok::trova_nel_path(), |p| Box::new(Grok::nuovo(p))),
    ];
    for (percorso, crea) in candidati {
        let Some(p) = percorso else { continue };
        // Gemini oggi non va sul Mac dell'utente (UNSUPPORTED_CLIENT, contratto §7): solo a richiesta.
        if p.file_name().is_some_and(|n| n == "gemini")
            && std::env::var_os("XBOT_WORKER_GEMINI").is_none_or(|v| v != "1")
        {
            continue;
        }
        let a = crea(&p);
        let modelli = match tokio::time::timeout(Duration::from_secs(90), a.modelli()).await {
            Ok(Ok(m)) => m,
            Ok(Err(e)) => {
                tracing::warn!(provider = a.id(), errore = %e, "modelli non letti");
                Vec::new()
            }
            Err(_) => {
                tracing::warn!(provider = a.id(), "modelli: troppo lento");
                Vec::new()
            }
        };
        let account = leggi_account(a.as_ref()).await;
        trovati.push(ProviderMacchina {
            account,
            id: a.id().to_string(),
            nome: a.nome().to_string(),
            versione: versione(&p).await,
            programma: p.to_string_lossy().into_owned(),
            modelli: modelli
                .into_iter()
                .map(|m| ModelloMacchina {
                    id: m.id,
                    etichetta: m.etichetta,
                    dettaglio: m.dettaglio,
                    livelli: m.livelli,
                })
                .collect(),
        });
    }
    trovati
}
