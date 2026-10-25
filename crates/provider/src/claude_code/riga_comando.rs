//! La riga di comando di `claude` per un turno e la riga utente in `stream-json`.
//!
//! Il modo dei permessi (2.L-quater, regola dell'utente del 2026-10-25): sulla macchina con
//! permesso «sempre» l'assistente agisce senza chiedere (`bypassPermissions`), con «mai» solo
//! piano (`plan`), con «chiedi» decide l'utente in chat attraverso lo strumento `permesso`
//! (`default` + `--permission-prompt-tool`): mai il messaggio interno di Claude Code.

use std::ffi::OsString;

use super::LIVELLI;
use crate::RichiestaTurno;

/// Il modo dei permessi quando la richiesta non ne chiede uno e non c'è lo strumento.
pub const PERMESSI_PREDEFINITI: &str = "acceptEdits";
const MODI_PERMESSI: [&str; 7] = [
    "acceptEdits",
    "default",
    "auto",
    "bypassPermissions",
    "manual",
    "dontAsk",
    "plan",
];

/// Il modo dei permessi del turno e lo strumento che chiede all'utente (se c'è).
///
/// `permessi` può portare il permesso della macchina (`always` | `never` | `ask`) o un modo
/// di Claude Code. `always`/`bypassPermissions` e `never`/`plan` non chiedono mai; negli altri
/// casi, se il turno ha lo strumento dei permessi, chiede quello.
pub fn modo_permessi<'a>(
    permessi: Option<&'a str>,
    strumento: Option<&'a str>,
) -> (&'a str, Option<&'a str>) {
    match permessi {
        Some("always" | "bypassPermissions") => ("bypassPermissions", None),
        Some("never" | "plan") => ("plan", None),
        p => match strumento {
            Some(t) => ("default", Some(t)),
            None => match p {
                Some("ask") => ("default", None),
                Some(m) if MODI_PERMESSI.contains(&m) => (m, None),
                _ => (PERMESSI_PREDEFINITI, None),
            },
        },
    }
}

/// Gli argomenti di `claude` per una richiesta.
pub fn argomenti(r: &RichiestaTurno) -> Vec<OsString> {
    let mut a: Vec<OsString> = [
        "-p",
        "--output-format",
        "stream-json",
        "--input-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--strict-mcp-config",
        "--settings",
        r#"{"disableAllHooks":true}"#,
    ]
    .iter()
    .map(OsString::from)
    .collect();
    let mut coppia = |k: &str, v: &str| {
        a.push(k.into());
        a.push(v.into());
    };
    if let Some(id) = r.sessione_esterna_id.as_deref().filter(|s| !s.is_empty()) {
        coppia("--resume", id);
    }
    if let Some(m) = r.modello.as_deref().filter(|s| !s.is_empty()) {
        coppia("--model", m);
    }
    if let Some(l) = r.livello.as_deref().filter(|l| LIVELLI.contains(l)) {
        coppia("--effort", l);
    }
    let strumento = r.mcp.as_ref().and_then(|m| m.strumento_permessi.as_deref());
    if let Some(m) = &r.mcp {
        coppia("--mcp-config", &m.file.to_string_lossy());
    }
    let (modo, chiedi) = modo_permessi(r.permessi.as_deref(), strumento);
    coppia("--permission-mode", modo);
    if let Some(t) = chiedi {
        coppia("--permission-prompt-tool", t);
    }
    if let Some(s) = r.sistema.as_deref().filter(|s| !s.trim().is_empty()) {
        // Un NUL nell'argomento impedisce di lanciare il processo: si toglie sempre.
        coppia("--append-system-prompt", &s.replace('\0', ""));
    }
    a
}

/// La riga utente in `stream-json`. Gli allegati passano come percorsi in coda al testo.
pub fn messaggio_utente(r: &RichiestaTurno) -> String {
    let mut testo = r.testo.clone();
    for p in &r.allegati {
        testo.push_str(&format!("\n\n[allegato: {}]", p.display()));
    }
    let v = serde_json::json!({
        "type": "user",
        "message": { "role": "user", "content": testo }
    });
    v.to_string()
}
