//! `scrivi_memoria` (2026-10-25): la memoria è una sola, quella dell'utente. Gli agenti di fuori
//! (altri client MCP) ci scrivono lo stato di avanzamento attraverso il canale MCP, solo
//! dentro la cartella della memoria (`memoria.cartella`) e solo file `.md`.

use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value};
use xbot_db::impostazioni;

use super::strumenti::Esito;
use crate::Stato;

/// La cartella della memoria comune: l'impostazione `memoria.cartella`.
async fn radice(m: &Stato) -> Result<PathBuf, String> {
    m.db.esegui(|c| impostazioni::leggi(c, "memoria.cartella"))
        .await
        .map_err(|e| e.testo)?
        .and_then(|v| v.as_str().map(PathBuf::from))
        .filter(|p| p.is_dir())
        .ok_or_else(|| "manca la cartella della memoria (impostazione memoria.cartella)".into())
}

/// Il percorso dentro la radice: relativo, senza `..`, con estensione `.md`.
fn dentro(radice: &Path, percorso: &str) -> Result<PathBuf, String> {
    let p = Path::new(percorso.trim().trim_start_matches('/'));
    if p.as_os_str().is_empty() || p.extension().and_then(|e| e.to_str()) != Some("md") {
        return Err(
            "il percorso vuole un file .md dentro la memoria (es. «Progetti/Esempio/Stato.md»)"
                .into(),
        );
    }
    if p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err("il percorso esce dalla memoria".into());
    }
    Ok(radice.join(p))
}

fn arg<'a>(a: &'a Map<String, Value>, k: &str) -> &'a str {
    a.get(k).and_then(Value::as_str).unwrap_or_default()
}

/// `scrivi_memoria {percorso, testo, modo?: aggiungi|sostituisci, autore?}`.
pub async fn scrivi(m: &Stato, chi: &str, a: &Map<String, Value>) -> Esito {
    let testo = arg(a, "testo");
    if testo.trim().is_empty() {
        return Err("il testo è vuoto".into());
    }
    if testo.len() > 200_000 {
        return Err("testo troppo lungo (al massimo 200 kB)".into());
    }
    let radice = radice(m).await?;
    let file = dentro(&radice, arg(a, "percorso"))?;
    let sostituisci = arg(a, "modo") == "sostituisci";
    let autore = Some(arg(a, "autore").trim())
        .filter(|x| !x.is_empty())
        .unwrap_or(chi)
        .to_string();
    let ora = chrono_ora();
    let (f2, t2) = (file.clone(), testo.to_string());
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        use std::io::Write;
        if let Some(d) = f2.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("cartella: {e}"))?;
        }
        if sostituisci {
            std::fs::write(&f2, t2.as_bytes()).map_err(|e| format!("scrittura: {e}"))
        } else {
            let mut o = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&f2)
                .map_err(|e| format!("apertura: {e}"))?;
            write!(o, "\n\n## {ora} · {autore}\n\n{}\n", t2.trim_end())
                .map_err(|e| format!("scrittura: {e}"))
        }
    })
    .await
    .map_err(|e| e.to_string())??;
    let rel = file
        .strip_prefix(&radice)
        .unwrap_or(&file)
        .display()
        .to_string();
    tracing::info!(file = %rel, chi = %chi, sostituito = sostituisci, "memoria comune scritta");
    Ok(format!(
        "{} nella memoria comune: {rel}",
        if sostituisci {
            "Scritto"
        } else {
            "Aggiunto in coda"
        }
    ))
}

/// Data e ora locali (`AAAA-MM-GG HH:MM`), dal comando `date` come vuole la regola degli orari.
fn chrono_ora() -> String {
    std::process::Command::new("date")
        .arg("+%Y-%m-%d %H:%M")
        .env("TZ", "Europe/Rome")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn percorsi() {
        let r = Path::new("/m");
        assert_eq!(dentro(r, "A/B.md").unwrap(), PathBuf::from("/m/A/B.md"));
        assert_eq!(dentro(r, "/A.md").unwrap(), PathBuf::from("/m/A.md"));
        assert!(dentro(r, "../x.md").is_err());
        assert!(dentro(r, "A/../../x.md").is_err());
        assert!(dentro(r, "A/x.sh").is_err());
        assert!(dentro(r, "").is_err());
    }
}
