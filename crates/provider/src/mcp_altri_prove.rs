#![allow(clippy::unwrap_used)]
use super::*;
use crate::claude_code::mcp::ConfigMcp;

fn richiesta(server: Vec<ServerMcp>) -> RichiestaTurno {
    RichiestaTurno {
        sessione_id: "01SESSIONE".into(),
        mcp: Some(ConfigMcp {
            server,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn due() -> Vec<ServerMcp> {
    vec![
        ServerMcp::Stdio {
            nome: SERVER_INTERNO.into(),
            comando: "/bin/xbot-server".into(),
            argomenti: vec![],
            ambiente: vec![],
        },
        ServerMcp::Stdio {
            nome: "posta_imap".into(),
            comando: "python3".into(),
            argomenti: vec!["~/strumenti/posta_mcp.py".into()],
            ambiente: vec![("TOKEN".into(), SegretoTesto::nuovo("a\"b"))],
        },
        ServerMcp::Http {
            nome: "odoo".into(),
            url: "https://o.esempio.it/mcp".into(),
            intestazioni: vec![("Authorization".into(), SegretoTesto::nuovo("Bearer t"))],
        },
    ]
}

fn cartella(nome: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("xbot-mcp-altri-{nome}-{}", xbot_core::nuovo_id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn nomi_per_grok() {
    assert_eq!(nome_grok("posta_imap"), "posta_imap");
    assert_eq!(nome_grok("2fa"), "c2fa");
    assert_eq!(nome_grok("a__b c"), "a_b_c");
    // 2.L-bis: il nome vero del catalogo dava `Posta_IMAP_` e `Posta_IMAP___caselle` in Grok.
    assert_eq!(nome_grok("Posta (IMAP)"), "posta_imap");
    assert_eq!(
        strumento_grok("Posta (IMAP)", "caselle"),
        "posta_imap__caselle"
    );
    assert_eq!(nome_grok("(x)"), "x");
    assert_eq!(nome_grok("__"), "c");
    let v = json_cursor(&[&ServerMcp::Stdio {
        nome: "Posta (IMAP)".into(),
        comando: "python3".into(),
        argomenti: vec![],
        ambiente: vec![],
    }]);
    assert!(v["mcpServers"]["posta_imap"].is_object(), "{v}");
    assert_eq!(strumento_grok("odoo", "cerca"), "odoo__cerca");
}

#[test]
fn toml_di_grok_senza_il_server_interno() {
    let r = richiesta(due());
    let t = toml_grok(&collegamenti(&r), None);
    let con_casa = toml_grok(&collegamenti(&r), Some(Path::new("/Users/utente")));
    assert!(
        con_casa.contains("env = { \"HOME\" = \"/Users/utente\", \"TOKEN\""),
        "{con_casa}"
    );
    assert!(!t.contains("xbot-server"), "{t}");
    assert!(
        t.contains("[mcp_servers.posta_imap]\ncommand = \"python3\""),
        "{t}"
    );
    assert!(t.contains("/strumenti/posta_mcp.py\"]"), "{t}");
    assert!(!t.contains("\"~/"), "la casa si espande: {t}");
    assert!(t.contains("env = { \"TOKEN\" = \"a\\\"b\" }"), "{t}");
    assert!(t.contains("[mcp_servers.odoo]\nurl = \"https://o.esempio.it/mcp\"\nheaders = { \"Authorization\" = \"Bearer t\" }"), "{t}");
}

#[test]
fn casa_della_sessione_con_i_collegamenti() {
    let base = cartella("grok");
    let home = cartella("home");
    // Senza collegamenti: la casa di sempre.
    let v = ambiente_grok_in(&base, &home, &richiesta(Vec::new())).unwrap();
    assert!(
        v.iter()
            .any(|(k, c)| k == "GROK_HOME" && Path::new(c) == base)
    );
    let r = richiesta(due());
    let v = ambiente_grok_in(&base, &home, &r).unwrap();
    let casa = base.join("sessioni").join("01SESSIONE");
    assert!(
        v.iter()
            .any(|(k, c)| k == "GROK_HOME" && Path::new(c) == casa),
        "{v:?}"
    );
    let t = std::fs::read_to_string(casa.join("config.toml")).unwrap();
    assert!(
        t.contains("[compat.claude]") && t.contains("[mcp_servers.odoo]"),
        "{t}"
    );
    assert!(casa.join("requirements.toml").is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let m = std::fs::metadata(casa.join("config.toml"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(m & 0o777, 0o600);
    }
    assert_ne!(firma(&r), firma(&richiesta(Vec::new())));
}

#[test]
fn mcp_json_di_cursor_e_file_dell_utente() {
    let d = cartella("cursor");
    let r = richiesta(due());
    assert!(scrivi_cursor(&d, &r).unwrap());
    let f = d.join(".cursor/mcp.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
    assert_eq!(v[SEGNO], true);
    assert!(v["mcpServers"].get(SERVER_INTERNO).is_none());
    assert_eq!(v["mcpServers"]["posta_imap"]["command"], "python3");
    assert_eq!(
        v["mcpServers"]["odoo"]["headers"]["Authorization"],
        "Bearer t"
    );
    assert!(
        crate::cursor_agent::argomenti(&r)
            .iter()
            .any(|a| a == "--approve-mcps")
    );
    // Senza collegamenti il nostro file si toglie e `--approve-mcps` non c'è.
    let vuota = richiesta(Vec::new());
    assert!(scrivi_cursor(&d, &vuota).unwrap());
    assert!(!f.exists());
    assert!(
        !crate::cursor_agent::argomenti(&vuota)
            .iter()
            .any(|a| a == "--approve-mcps")
    );
    // Un file dell'utente resta com'è.
    std::fs::write(&f, "{\"mcpServers\":{\"suo\":{\"command\":\"x\"}}}").unwrap();
    assert!(!scrivi_cursor(&d, &r).unwrap());
    assert!(std::fs::read_to_string(&f).unwrap().contains("suo"));
}

/// 2.L-ter: `agent` legge `.cursor/mcp.json` dalla radice git. Una cartella di sessione di XBot
/// dentro un repository diventa un repository suo; una cartella dell'utente usa la radice di sopra.
#[tokio::test]
async fn radice_di_cursor_e_repository_di_sopra() {
    let fuori = cartella("radice-fuori");
    assert_eq!(radice_cursor(&fuori, true).await, fuori);
    assert!(!fuori.join(".git").exists());

    let repo = std::fs::canonicalize(cartella("radice-repo")).unwrap();
    let ok = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .unwrap();
    assert!(ok.success());
    let sua = repo.join("dell-utente");
    assert_eq!(radice_cursor(&sua, false).await, repo);
    assert!(!sua.join(".git").exists());

    let nostra = repo.join("sessioni").join("01SESSIONE");
    assert_eq!(radice_cursor(&nostra, true).await, nostra);
    assert!(nostra.join(".git").is_dir());
    // La seconda volta è già la radice: nessun cambio.
    assert_eq!(radice_cursor(&nostra, true).await, nostra);
}
