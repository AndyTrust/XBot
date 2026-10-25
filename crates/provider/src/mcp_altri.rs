//! Fase 2.L: i collegamenti del turno anche per Grok e Cursor. Il motore e il lavoratore danno
//! la stessa `ConfigMcp` di Claude Code (senza il server interno di XBot, che serve solo a Claude
//! Code per i permessi); qui diventa:
//! - per Grok, le sezioni `[mcp_servers.<nome>]` del `config.toml` di una casa della sessione
//!   (`<casa>/sessioni/<sessione>`, così due sessioni non si pestano);
//! - per Cursor, `.cursor/mcp.json` nella cartella del turno (`mcpServers`), con `--approve-mcps`.
//!
//! I permessi degli strumenti dei collegamenti seguono il permesso della macchina: nessun socket.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use xbot_core::{Errore, SegretoTesto};

use crate::RichiestaTurno;
use crate::claude_code::mcp::{SERVER_INTERNO, ServerMcp};
use crate::grok::casa;
use crate::prova_mcp::espandi_casa;

/// Il segno che il file l'ha scritto XBot (un `.cursor/mcp.json` dell'utente non si tocca).
pub const SEGNO: &str = "_scritto_da_xbot";

/// I collegamenti del turno (senza il server interno).
pub fn collegamenti(r: &RichiestaTurno) -> Vec<&ServerMcp> {
    r.mcp
        .as_ref()
        .map(|c| {
            c.server
                .iter()
                .filter(|s| s.nome() != SERVER_INTERNO)
                .collect()
        })
        .unwrap_or_default()
}

/// Un nome che Grok e Cursor accettano: `[a-z][a-z0-9_-]*` minuscolo, senza `__` e senza `_` o
/// `-` in fondo. Grok chiama gli strumenti `<server>__<strumento>`: con `Posta_IMAP_` veniva
/// `Posta_IMAP___caselle`, «invalid or ambiguous qualified name», e il server restava con zero
/// strumenti (2.L-bis, `events.jsonl` della sessione vera). Cursor rifiuta i nomi con spazi e
/// parentesi («namespace "Posta (IMAP)" not found»).
pub fn nome_grok(n: &str) -> String {
    let mut s: String = n
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    while s.contains("__") {
        s = s.replace("__", "_");
    }
    let s = s.trim_matches(|c| c == '_' || c == '-');
    match s.chars().next() {
        Some(c) if c.is_ascii_alphabetic() => s.to_string(),
        _ => format!("c{s}"),
    }
}

fn tabella(coppie: &[(String, SegretoTesto)]) -> String {
    let voci: Vec<String> = coppie
        .iter()
        .map(|(k, v)| format!("{} = {}", casa::toml_testo(k), casa::toml_testo(v.esponi())))
        .collect();
    format!("{{ {} }}", voci.join(", "))
}

/// Le sezioni `[mcp_servers.<nome>]` per il `config.toml` di Grok. Grok gira con `HOME` = casa
/// isolata: un server stdio riceve la `HOME` vera (`home`), altrimenti non trova i suoi file
/// (per esempio un file `~/.env` della posta, visto il 2026-10-25).
pub fn toml_grok(server: &[&ServerMcp], home: Option<&Path>) -> String {
    let mut t = String::new();
    for s in server {
        t.push_str(&format!("\n[mcp_servers.{}]\n", nome_grok(s.nome())));
        match s {
            ServerMcp::Stdio {
                comando,
                argomenti,
                ambiente,
                ..
            } => {
                let a: Vec<String> = argomenti
                    .iter()
                    .map(|a| casa::toml_testo(&espandi_casa(a)))
                    .collect();
                let mut amb: Vec<(String, SegretoTesto)> = Vec::new();
                if let Some(h) = home {
                    amb.push(("HOME".into(), SegretoTesto::nuovo(h.to_string_lossy())));
                }
                amb.extend(ambiente.iter().filter(|(k, _)| k != "HOME").cloned());
                t.push_str(&format!(
                    "command = {}\nargs = [{}]\nenv = {}\nstartup_timeout_sec = 60\n",
                    casa::toml_testo(&espandi_casa(comando)),
                    a.join(", "),
                    tabella(&amb)
                ));
            }
            ServerMcp::Http {
                url, intestazioni, ..
            } => t.push_str(&format!(
                "url = {}\nheaders = {}\n",
                casa::toml_testo(url),
                tabella(intestazioni)
            )),
        }
    }
    t
}

/// L'ambiente di grok per il turno: la casa di sempre se non ci sono collegamenti, altrimenti
/// una casa della sessione con gli stessi collegamenti a `auth.json` e `sessions` e i server
/// MCP in fondo al `config.toml`.
pub fn ambiente_grok(base: &Path, r: &RichiestaTurno) -> Result<Vec<(String, String)>, Errore> {
    let home = std::env::var_os("HOME").map_or_else(std::env::temp_dir, PathBuf::from);
    ambiente_grok_in(base, &home, r)
}

/// [`ambiente_grok`] con la `HOME` vera data (per le prove).
pub fn ambiente_grok_in(
    base: &Path,
    home: &Path,
    r: &RichiestaTurno,
) -> Result<Vec<(String, String)>, Errore> {
    let server = collegamenti(r);
    let errore =
        |c: &Path, e: std::io::Error| Errore::interno(format!("casa di grok {}: {e}", c.display()));
    if server.is_empty() {
        return casa::prepara_in(base, home).map_err(|e| errore(base, e));
    }
    let sessione: String = r
        .sessione_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    let c = base.join("sessioni").join(sessione);
    let v = casa::prepara_in(&c, home).map_err(|e| errore(&c, e))?;
    let testo = format!(
        "{}{}",
        casa::configurazione(home),
        toml_grok(&server, Some(home))
    );
    crate::claude_code::mcp::scrivi_privato(&c.join("config.toml"), testo.as_bytes())?;
    Ok(v)
}

/// Un testo che cambia quando cambiano i collegamenti (per la firma del processo caldo).
pub fn firma(r: &RichiestaTurno) -> String {
    toml_grok(&collegamenti(r), None)
}

/// Il JSON di `.cursor/mcp.json`.
pub fn json_cursor(server: &[&ServerMcp]) -> Value {
    let mappa = |c: &[(String, SegretoTesto)]| -> Value {
        Value::Object(
            c.iter()
                .map(|(k, v)| (k.clone(), Value::from(v.esponi())))
                .collect::<Map<String, Value>>(),
        )
    };
    let mut m = Map::new();
    for s in server {
        let v = match s {
            ServerMcp::Stdio {
                comando,
                argomenti,
                ambiente,
                ..
            } => json!({
                "command": espandi_casa(comando),
                "args": argomenti.iter().map(|a| espandi_casa(a)).collect::<Vec<_>>(),
                "env": mappa(ambiente),
            }),
            ServerMcp::Http {
                url, intestazioni, ..
            } => json!({ "url": url, "headers": mappa(intestazioni) }),
        };
        m.insert(nome_grok(s.nome()), v);
    }
    json!({ "mcpServers": m, SEGNO: true })
}

/// Scrive `.cursor/mcp.json` nella cartella del turno (0600). `Ok(false)`: c'è già un file di
/// L'utente (senza il segno di XBot) e non si tocca; `Ok(true)`: scritto (o tolto, senza collegamenti).
pub fn scrivi_cursor(cartella: &Path, r: &RichiestaTurno) -> Result<bool, Errore> {
    let file = cartella.join(".cursor").join("mcp.json");
    let nostro = match std::fs::read_to_string(&file) {
        Ok(t) => serde_json::from_str::<Value>(&t).is_ok_and(|v| v[SEGNO] == true),
        Err(_) => true,
    };
    if !nostro {
        return Ok(false);
    }
    let server = collegamenti(r);
    if server.is_empty() {
        if file.exists() {
            std::fs::remove_file(&file)
                .map_err(|e| Errore::interno(format!("{}: {e}", file.display())))?;
        }
        return Ok(true);
    }
    let testo = json_cursor(&server).to_string();
    crate::claude_code::mcp::scrivi_privato(&file, testo.as_bytes())?;
    Ok(true)
}

/// La cartella dove `agent` cerca `.cursor/mcp.json` (2.L-ter, prova a mano del 2026-10-25):
/// non la cartella del turno ma la radice git che la contiene. Le cartelle delle sessioni stanno
/// possono stare dentro il worktree di un repository (anche quello di XBot): il file del turno
/// non veniva letto. Una cartella di sessione di XBot (`propria`) dentro un repository diventa un
/// repository suo (`git init`, che tiene anche i comandi git del bot fuori dal repository di
/// sopra); una cartella dell'utente dentro un repository usa la sua radice. Senza git: la cartella.
pub async fn radice_cursor(cartella: &Path, propria: bool) -> PathBuf {
    let Some(radice) = radice_git(cartella).await else {
        return cartella.to_path_buf();
    };
    let vera = std::fs::canonicalize(cartella).unwrap_or_else(|_| cartella.to_path_buf());
    if radice == vera || !propria {
        return radice;
    }
    let iniziata = git(cartella, &["init", "-q"]).await.is_some();
    if iniziata && radice_git(cartella).await.as_deref() == Some(vera.as_path()) {
        vera
    } else {
        radice
    }
}

async fn radice_git(cartella: &Path) -> Option<PathBuf> {
    let t = git(cartella, &["rev-parse", "--show-toplevel"]).await?;
    let t = t.trim();
    (!t.is_empty()).then(|| PathBuf::from(t))
}

/// Un comando git nella cartella (creata se manca); `None` se git manca o fallisce.
async fn git(cartella: &Path, argomenti: &[&str]) -> Option<String> {
    tokio::fs::create_dir_all(cartella).await.ok()?;
    let uscita = tokio::process::Command::new("git")
        .arg("-C")
        .arg(cartella)
        .args(argomenti)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .ok()?;
    uscita
        .status
        .success()
        .then(|| String::from_utf8_lossy(&uscita.stdout).into_owned())
}

/// La nota in chat per Grok e Cursor.
pub const NOTA_PERMESSI: &str =
    "i comandi e gli strumenti dei collegamenti seguono il permesso della macchina";

/// Per i test: il nome che Grok dà a uno strumento di un collegamento.
pub fn strumento_grok(server: &str, strumento: &str) -> String {
    format!("{}__{strumento}", nome_grok(server))
}

#[cfg(test)]
#[path = "mcp_altri_prove.rs"]
mod prove;
