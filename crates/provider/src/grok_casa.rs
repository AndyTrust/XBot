//! La casa isolata di grok (Fase 2.G-bis). Senza, grok carica tutto quello che l'utente ha per
//! Claude Code e per sé: i server MCP di `~/.claude.json` e dei plugin (32 s di avvio con `rea`
//! e `hermes` in timeout), gli hook di `~/.claude/settings.json` (fino a 30 s dopo ogni turno),
//! le skill, il `CLAUDE.md` globale, le regole di permesso di `~/.claude/settings.local.json`,
//! la memoria `~/.grok/memory-v2` e l'indice del codice. Misura vera nel contratto §19.
//!
//! La casa è una cartella di XBot (`<worker>/grok-home` sul Mac, `$XBOT_DATI/grok-home` sul
//! server) passata a grok sia come `GROK_HOME` sia come `HOME`: grok cerca `~/.claude*`,
//! `~/.agents` e `~/.cursor` sotto `HOME`, quindi lì non trova niente. I comandi che grok lancia
//! ricevono la `HOME` vera da `[shell_environment_policy] set`. Dentro ci sono:
//! - `auth.json`: collegamento simbolico a `~/.grok/auth.json` (provato con grok 1.0.70: il
//!   rinnovo del token scrive attraverso il collegamento e il collegamento resta). Se grok lo
//!   sostituisce con un file vero, il file più recente torna in `~/.grok/auth.json` e il
//!   collegamento si rifà; se il collegamento non si può fare, una copia 0600.
//! - `sessions`: collegamento a `~/.grok/sessions`, così le sessioni di prima si riprendono.
//! - `config.toml` e `requirements.toml` scritti da XBot a ogni avvio.

use std::io;
use std::path::{Path, PathBuf};

/// Il `requirements.toml` della casa: è un livello di regole, l'unico dove
/// `allow_managed_hooks_only` spegne anche gli hook dei plugin e dei file di Claude e Cursor.
pub const REQUISITI: &str = "# Scritto da XBot a ogni avvio di grok (Fase 2.G-bis): nessun hook.\n\
allow_managed_hooks_only = true\n";

/// La casa predefinita: `$XBOT_DATI/grok-home` se c'è, altrimenti `~/.xbot/grok-home`
/// (il lavoratore passa la sua, `<cartella del lavoratore>/grok-home`).
pub fn predefinita() -> PathBuf {
    xbot_core::cartella_dati_predefinita().join("grok-home")
}

/// La `HOME` di XBot (non quella della casa).
fn home_vera() -> PathBuf {
    std::env::var_os("HOME").map_or_else(std::env::temp_dir, PathBuf::from)
}

/// Una stringa TOML fra virgolette.
pub(crate) fn toml_testo(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            c if c.is_control() => o.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Il `config.toml` della casa. `home` è la `HOME` vera, che va ai comandi di grok.
pub fn configurazione(home: &Path) -> String {
    let mut c = String::from(
        "# Scritto da XBot a ogni avvio di grok (Fase 2.G-bis): non modificare a mano.\n\
[cli]\nauto_update = false\nshow_tips = false\n\n\
[features]\ntelemetry = false\nfeedback = false\ncodebase_indexing = false\n\
turn_summary = false\nsession_recap = false\ntitle_refresh = false\ncampaigns = false\n\
managed_config = false\n\n\
[memory]\nenabled = false\n\n[memory_v2]\nenabled = false\n\n\
[subagents]\nenabled = false\n\n",
    );
    for v in ["claude", "cursor"] {
        c.push_str(&format!("[compat.{v}]\n"));
        for k in ["skills", "rules", "agents", "mcps", "hooks", "sessions"] {
            c.push_str(&format!("{k} = false\n"));
        }
        c.push('\n');
    }
    c.push_str("[compat.codex]\nsessions = false\n\n");
    c.push_str(&format!(
        "[shell_environment_policy]\nset = {{ HOME = {} }}\n",
        toml_testo(&home.to_string_lossy())
    ));
    c
}

/// Le variabili che puntano grok alla casa (vanno in fondo all'ambiente, così vincono).
pub fn ambiente(casa: &Path) -> Vec<(String, String)> {
    let c = casa.to_string_lossy().into_owned();
    [
        ("HOME", c.as_str()),
        ("GROK_HOME", c.as_str()),
        ("GROK_MEMORY", "0"),
        ("GROK_SUBAGENTS", "0"),
        ("GROK_TELEMETRY_ENABLED", "0"),
        ("GROK_FEEDBACK_ENABLED", "0"),
        ("GROK_DISABLE_AUTOUPDATER", "1"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

#[cfg(unix)]
fn permessi(p: &Path, modo: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(modo))
}

#[cfg(not(unix))]
fn permessi(_: &Path, _: u32) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn collega(origine: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(origine, link)
}

#[cfg(not(unix))]
fn collega(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::other("collegamenti simbolici non disponibili"))
}

/// Scrive `testo` in `p` (0600) solo se è cambiato, passando da un file temporaneo.
fn scrivi_se_cambia(p: &Path, testo: &str) -> io::Result<()> {
    if std::fs::read_to_string(p).is_ok_and(|t| t == testo) {
        return Ok(());
    }
    let tmp = p.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, testo)?;
    permessi(&tmp, 0o600)?;
    std::fs::rename(&tmp, p)
}

fn modificato(p: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// `casa/auth.json` → `~/.grok/auth.json`. Un file vero più recente (grok lo ha riscritto al posto
/// del collegamento) torna prima nell'originale, così il token rinnovato non si perde.
fn prepara_accesso(casa: &Path, origine: &Path) -> io::Result<()> {
    let qui = casa.join("auth.json");
    let fonte = origine.join("auth.json");
    if let Ok(m) = std::fs::symlink_metadata(&qui) {
        if m.file_type().is_symlink() {
            if std::fs::read_link(&qui).is_ok_and(|d| d == fonte) && fonte.exists() {
                return Ok(());
            }
        } else if m.is_file() {
            let piu_nuovo = match (modificato(&qui), modificato(&fonte)) {
                (Some(a), Some(b)) => a > b,
                (Some(_), None) => true,
                _ => false,
            };
            if piu_nuovo {
                std::fs::create_dir_all(origine)?;
                std::fs::copy(&qui, &fonte)?;
                permessi(&fonte, 0o600)?;
            }
        }
        std::fs::remove_file(&qui)?;
    }
    if !fonte.exists() {
        // Senza accesso grok risponde «Not signed in»: l'adattatore lo dice all'utente.
        return Ok(());
    }
    match collega(&fonte, &qui) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => {
            tracing::warn!(errore = %e, "auth.json di grok: collegamento non riuscito, copio");
            std::fs::copy(&fonte, &qui)?;
            permessi(&qui, 0o600)
        }
    }
}

/// `casa/sessions` → `~/.grok/sessions` (le sessioni di grok restano dove erano).
fn prepara_sessioni(casa: &Path, origine: &Path) -> io::Result<()> {
    let qui = casa.join("sessions");
    if std::fs::symlink_metadata(&qui).is_ok() {
        return Ok(());
    }
    let fonte = origine.join("sessions");
    std::fs::create_dir_all(&fonte)?;
    match collega(&fonte, &qui) {
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists => {
            tracing::warn!(errore = %e, "sessioni di grok: collegamento non riuscito");
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Prepara la casa e dà le variabili per grok. `home` è la `HOME` vera (dove sta `~/.grok`).
pub fn prepara_in(casa: &Path, home: &Path) -> io::Result<Vec<(String, String)>> {
    std::fs::create_dir_all(casa)?;
    permessi(casa, 0o700)?;
    let origine = home.join(".grok");
    prepara_accesso(casa, &origine)?;
    prepara_sessioni(casa, &origine)?;
    scrivi_se_cambia(&casa.join("config.toml"), &configurazione(home))?;
    scrivi_se_cambia(&casa.join("requirements.toml"), REQUISITI)?;
    Ok(ambiente(casa))
}

/// [`prepara_in`] con la `HOME` di XBot.
pub fn prepara(casa: &Path) -> io::Result<Vec<(String, String)>> {
    prepara_in(casa, &home_vera())
}

#[cfg(test)]
mod prove {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn cartella(nome: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("xbot-prova-2gbis-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn casa_completa() {
        let d = cartella("casa");
        let home = d.join("home");
        std::fs::create_dir_all(home.join(".grok")).unwrap();
        std::fs::write(home.join(".grok/auth.json"), "{\"a\":1}").unwrap();
        let casa = d.join("grok-home");
        let amb = prepara_in(&casa, &home).unwrap();
        let v: std::collections::HashMap<_, _> =
            amb.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
        assert_eq!(v["HOME"], casa.to_string_lossy());
        assert_eq!(v["GROK_HOME"], casa.to_string_lossy());
        assert_eq!(v["GROK_MEMORY"], "0");
        assert_eq!(v["GROK_SUBAGENTS"], "0");
        assert_eq!(v["GROK_TELEMETRY_ENABLED"], "0");
        let link = std::fs::read_link(casa.join("auth.json")).unwrap();
        assert_eq!(link, home.join(".grok/auth.json"));
        assert_eq!(
            std::fs::read_link(casa.join("sessions")).unwrap(),
            home.join(".grok/sessions")
        );
        let conf = std::fs::read_to_string(casa.join("config.toml")).unwrap();
        for k in [
            "codebase_indexing = false",
            "telemetry = false",
            "[compat.claude]",
            "mcps = false",
            "hooks = false",
        ] {
            assert!(conf.contains(k), "{k}");
        }
        assert!(conf.contains(&format!("HOME = \"{}\"", home.display())));
        let req = std::fs::read_to_string(casa.join("requirements.toml")).unwrap();
        assert!(req.contains("allow_managed_hooks_only = true"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let m = std::fs::metadata(&casa).unwrap().permissions().mode() & 0o777;
            assert_eq!(m, 0o700);
            let m = std::fs::metadata(casa.join("config.toml"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(m, 0o600);
        }
        // Una seconda volta non cambia niente.
        prepara_in(&casa, &home).unwrap();
        assert!(
            std::fs::symlink_metadata(casa.join("auth.json"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn accesso_riscritto_da_grok_torna_nell_originale() {
        let d = cartella("accesso");
        let home = d.join("home");
        std::fs::create_dir_all(home.join(".grok")).unwrap();
        std::fs::write(home.join(".grok/auth.json"), "vecchio").unwrap();
        let casa = d.join("grok-home");
        std::fs::create_dir_all(&casa).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        // grok ha sostituito il collegamento con un file vero, più recente.
        std::fs::write(casa.join("auth.json"), "rinnovato").unwrap();
        prepara_in(&casa, &home).unwrap();
        assert_eq!(
            std::fs::read_to_string(home.join(".grok/auth.json")).unwrap(),
            "rinnovato"
        );
        assert!(
            std::fs::symlink_metadata(casa.join("auth.json"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_to_string(casa.join("auth.json")).unwrap(),
            "rinnovato"
        );
    }

    #[test]
    fn senza_accesso_nessun_collegamento() {
        let d = cartella("senza");
        let home = d.join("home");
        let casa = d.join("grok-home");
        prepara_in(&casa, &home).unwrap();
        assert!(std::fs::symlink_metadata(casa.join("auth.json")).is_err());
    }

    #[test]
    fn testo_toml() {
        assert_eq!(toml_testo("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }
}
