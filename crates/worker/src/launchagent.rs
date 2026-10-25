//! `xbot-worker install-launchagent`: scrive `~/Library/LaunchAgents/<etichetta>.plist`
//! (etichetta da `XBOT_ETICHETTA_WORKER`, predefinita `it.xbot.worker`; sempre acceso,
//! riparte se cade, log in `<cartella>/log/`). Con `--carica` lo avvia anche.

use std::path::{Path, PathBuf};

use xbot_core::{Errore, Risposta};

/// L'etichetta predefinita del LaunchAgent.
pub const ETICHETTA: &str = "it.xbot.worker";

/// L'etichetta del LaunchAgent: `XBOT_ETICHETTA_WORKER` se c'è (solo lettere, cifre, `.`, `-`,
/// `_`), altrimenti `it.xbot.worker`.
pub fn etichetta() -> String {
    std::env::var("XBOT_ETICHETTA_WORKER")
        .ok()
        .map(|e| e.trim().to_string())
        .filter(|e| etichetta_valida(e))
        .unwrap_or_else(|| ETICHETTA.into())
}

fn etichetta_valida(e: &str) -> bool {
    !e.is_empty()
        && e.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn xml(t: &str) -> String {
    t.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Il testo del plist.
pub fn plist(programma: &Path, cartella: &Path, path: &str, home: &str) -> String {
    plist_con_etichetta(&etichetta(), programma, cartella, path, home)
}

/// Il testo del plist con un'etichetta data.
pub fn plist_con_etichetta(
    etichetta: &str,
    programma: &Path,
    cartella: &Path,
    path: &str,
    home: &str,
) -> String {
    let log = cartella.join("log");
    let s = |p: &Path| xml(&p.to_string_lossy());
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{etic}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{prog}</string>
    <string>run</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key><string>{path}</string>
    <key>HOME</key><string>{home}</string>
    <key>XBOT_WORKER_CARTELLA</key><string>{cart}</string>
  </dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>10</integer>
  <key>ProcessType</key><string>Interactive</string>
  <key>StandardOutPath</key><string>{out}</string>
  <key>StandardErrorPath</key><string>{err}</string>
</dict>
</plist>
"#,
        etic = xml(etichetta),
        prog = s(programma),
        path = xml(path),
        home = xml(home),
        cart = s(cartella),
        out = s(&log.join("worker.log")),
        err = s(&log.join("worker.err.log")),
    )
}

pub fn file_plist() -> PathBuf {
    std::env::var_os("HOME")
        .map_or_else(std::env::temp_dir, PathBuf::from)
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", etichetta()))
}

/// Scrive il plist (e la cartella dei log). Restituisce il percorso.
pub fn installa(programma: &Path, cartella: &Path) -> Risposta<PathBuf> {
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
    let home = std::env::var("HOME").unwrap_or_default();
    let file = file_plist();
    let err = |e: std::io::Error| Errore::interno(format!("{}: {e}", file.display()));
    std::fs::create_dir_all(cartella.join("log")).map_err(err)?;
    if let Some(d) = file.parent() {
        std::fs::create_dir_all(d).map_err(err)?;
    }
    std::fs::write(&file, plist(programma, cartella, &path, &home)).map_err(err)?;
    Ok(file)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn plist_valido() {
        let p = plist(
            Path::new("/opt/x & y/xbot-worker"),
            Path::new("/c"),
            "/bin",
            "/home/b",
        );
        assert!(p.contains("<string>/opt/x &amp; y/xbot-worker</string>"));
        assert!(p.contains("<string>run</string>") && p.contains(&etichetta()));
        assert!(p.contains("/c/log/worker.log"));
        // 2.L: con `Background` macOS mette il lavoratore e i suoi figli a priorità minima.
        assert!(p.contains("<key>ProcessType</key><string>Interactive</string>"));
        let q = plist_con_etichetta(
            "it.prova.lavoratore",
            Path::new("/w"),
            Path::new("/c"),
            "/bin",
            "/h",
        );
        assert!(q.contains("<key>Label</key><string>it.prova.lavoratore</string>"));
        assert!(
            etichetta_valida("it.xbot.worker") && !etichetta_valida("a b") && !etichetta_valida("")
        );
    }
}
