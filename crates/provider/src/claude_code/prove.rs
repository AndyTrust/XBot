//! Prove dell'adattatore `claude_code` senza processo.

use super::*;
use std::ffi::OsString;

fn testi(a: &[OsString]) -> Vec<String> {
    a.iter().map(|s| s.to_string_lossy().into_owned()).collect()
}

#[test]
fn argomenti_completi() {
    let r = RichiestaTurno {
        sessione_id: "S".into(),
        sessione_esterna_id: Some("abc".into()),
        modello: Some("claude-haiku-5-5".into()),
        livello: Some("high".into()),
        sistema: Some("sei Atena".into()),
        testo: "ciao".into(),
        ..Default::default()
    };
    let a = testi(&argomenti(&r));
    let dopo = |k: &str| a[a.iter().position(|x| x == k).unwrap() + 1].clone();
    assert_eq!(a[0], "-p");
    assert!(a.contains(&"--strict-mcp-config".to_string()));
    assert_eq!(dopo("--settings"), r#"{"disableAllHooks":true}"#);
    assert_eq!(dopo("--resume"), "abc");
    assert_eq!(dopo("--model"), "claude-haiku-5-5");
    assert_eq!(dopo("--effort"), "high");
    assert_eq!(dopo("--permission-mode"), "acceptEdits");
    assert_eq!(dopo("--append-system-prompt"), "sei Atena");
}

#[test]
fn argomenti_minimi_e_permessi() {
    let r = RichiestaTurno {
        livello: Some("altissimo".into()),
        permessi: Some("plan".into()),
        ..Default::default()
    };
    let a = testi(&argomenti(&r));
    for k in ["--resume", "--model", "--effort", "--append-system-prompt"] {
        assert!(!a.contains(&k.to_string()), "{k}");
    }
    assert!(a.ends_with(&["--permission-mode".into(), "plan".into()]));
    let r = RichiestaTurno {
        permessi: Some("inventato".into()),
        ..Default::default()
    };
    assert!(testi(&argomenti(&r)).contains(&PERMESSI_PREDEFINITI.to_string()));
}

#[test]
fn messaggio_in_stream_json() {
    let r = RichiestaTurno {
        testo: "di' \"ciao\"".into(),
        allegati: vec![PathBuf::from("/tmp/a.png")],
        ..Default::default()
    };
    let v: serde_json::Value = serde_json::from_str(&messaggio_utente(&r)).unwrap();
    assert_eq!(v["type"], "user");
    assert_eq!(v["message"]["role"], "user");
    assert_eq!(
        v["message"]["content"],
        "di' \"ciao\"\n\n[allegato: /tmp/a.png]"
    );
}

#[test]
fn cartella_della_sessione() {
    let c = ClaudeCode::nuovo("/x/claude").con_cartella_sessioni("/s");
    let r = RichiestaTurno {
        sessione_id: "S1".into(),
        ..Default::default()
    };
    assert_eq!(c.cartella_di(&r), PathBuf::from("/s/S1"));
    let r = RichiestaTurno {
        cartella_lavoro: Some("/lavoro".into()),
        ..r
    };
    assert_eq!(c.cartella_di(&r), PathBuf::from("/lavoro"));
    assert_eq!(c.programma(), "/x/claude");
}

#[test]
fn argomenti_con_mcp() {
    let r = RichiestaTurno {
        permessi: Some("acceptEdits".into()),
        mcp: Some(mcp::ConfigMcp {
            file: PathBuf::from("/t/mcp-config.json"),
            server: vec![],
            strumento_permessi: Some(mcp::STRUMENTO_PERMESSI.into()),
        }),
        ..Default::default()
    };
    let a = testi(&argomenti(&r));
    let dopo = |k: &str| a[a.iter().position(|x| x == k).unwrap() + 1].clone();
    assert_eq!(dopo("--mcp-config"), "/t/mcp-config.json");
    assert_eq!(dopo("--permission-mode"), "default");
    assert_eq!(dopo("--permission-prompt-tool"), "mcp__xbot__permesso");
    assert!(a.contains(&"--strict-mcp-config".to_string()));
}

#[test]
fn niente_nul_nel_testo_di_sistema() {
    let r = RichiestaTurno {
        sistema: Some("a\0b".into()),
        ..Default::default()
    };
    let a = testi(&argomenti(&r));
    assert!(a.contains(&"ab".to_string()));
}

#[test]
fn modo_dei_permessi_segue_la_macchina() {
    let t = Some("mcp__xbot__permesso");
    assert_eq!(
        modo_permessi(Some("always"), t),
        ("bypassPermissions", None)
    );
    assert_eq!(modo_permessi(Some("never"), t), ("plan", None));
    assert_eq!(modo_permessi(Some("ask"), t), ("default", t));
    assert_eq!(modo_permessi(Some("ask"), None), ("default", None));
    // Il server (VPS) non manda un modo: con lo strumento chiede all'utente come prima.
    assert_eq!(modo_permessi(None, t), ("default", t));
    assert_eq!(modo_permessi(None, None), (PERMESSI_PREDEFINITI, None));
    assert_eq!(modo_permessi(Some("dontAsk"), None), ("dontAsk", None));
}

#[test]
fn processo_caldo_nuovo_se_cambia_il_permesso() {
    let r = |p: &str| RichiestaTurno {
        permessi: Some(p.into()),
        ..Default::default()
    };
    let (prog, dir) = (Path::new("/x/claude"), Path::new("/s"));
    assert_ne!(firma(prog, dir, &r("always")), firma(prog, dir, &r("ask")));
    assert_eq!(firma(prog, dir, &r("ask")), firma(prog, dir, &r("ask")));
}
