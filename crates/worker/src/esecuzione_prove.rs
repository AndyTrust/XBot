//! Prove dell'esecuzione: richiesta del turno, allegati, modo dei permessi di Claude Code.

use super::*;
use crate::protocollo::DalServer;
use std::os::unix::fs::PermissionsExt;

/// Il frame `lavoro` di una routine come lo manda la VPS (sessione 01M5SAPF…, 2026-10-25),
/// con il permesso della macchina scelto dalla prova.
fn lavoro_routine(permesso: Option<&str>, chiedi: bool) -> Lavoro {
    let mut richiesta = serde_json::json!({
        "provider_id": "claude_code", "modello": "claude-sonnet-5-5", "livello": "medium",
        "sessione_esterna_id": "3fec0a98", "permessi": null, "chiedi_permessi": chiedi,
        "testo": "Con Read leggi ~/routine/x.md",
        "collegamenti": [{"nome": "posta_imap", "trasporto": "stdio", "comando": "python3",
            "argomenti": ["posta_mcp.py"], "intestazioni": [], "ambiente": []}]
    });
    if let Some(p) = permesso {
        richiesta["permesso_esecuzione"] = serde_json::json!(p);
    }
    let v = serde_json::json!({"tipo": "lavoro", "id": "L1", "generazione": 1,
        "sessione_id": "S", "richiesta": richiesta});
    let DalServer::Lavoro { id, richiesta, .. } = serde_json::from_value(v).unwrap() else {
        panic!("non è un lavoro")
    };
    Lavoro {
        id,
        generazione: 1,
        sessione_id: "S".into(),
        turno_id: Some("T".into()),
        richiesta: *richiesta,
    }
}

fn riga_di_claude(l: &Lavoro) -> Vec<String> {
    let conf = Config::predefinita();
    let gettone = claude_chiede(&l.richiesta).then_some("G");
    let (r, _) = richiesta_turno(&conf, l, gettone, Path::new("/tmp/x.sock"));
    xbot_provider::claude_code::argomenti(&r)
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect()
}

fn dopo(a: &[String], k: &str) -> Option<String> {
    a.iter().position(|x| x == k).map(|i| a[i + 1].clone())
}

#[test]
fn routine_su_macchina_sempre_agisce_senza_chiedere() {
    // Il difetto del 2026-10-25: `always` dava `acceptEdits` senza strumento, e Claude Code
    // negava Read e Bash con i suoi messaggi.
    for l in [
        lavoro_routine(Some("always"), false),
        lavoro_routine(None, false),
    ] {
        let a = riga_di_claude(&l);
        assert_eq!(
            dopo(&a, "--permission-mode").as_deref(),
            Some("bypassPermissions")
        );
        assert!(
            !a.contains(&"--permission-prompt-tool".to_string()),
            "{a:?}"
        );
        assert!(
            dopo(&a, "--mcp-config").is_some(),
            "collegamenti persi: {a:?}"
        );
    }
}

#[test]
fn routine_su_macchina_chiedi_passa_da_utente() {
    for l in [
        lavoro_routine(Some("ask"), true),
        lavoro_routine(None, true),
    ] {
        let a = riga_di_claude(&l);
        assert_eq!(dopo(&a, "--permission-mode").as_deref(), Some("default"));
        assert_eq!(
            dopo(&a, "--permission-prompt-tool").as_deref(),
            Some(mcp::STRUMENTO_PERMESSI)
        );
    }
    let a = riga_di_claude(&lavoro_routine(Some("never"), true));
    assert_eq!(dopo(&a, "--permission-mode").as_deref(), Some("plan"));
    assert!(
        !a.contains(&"--permission-prompt-tool".to_string()),
        "{a:?}"
    );
}

#[test]
fn altri_client_tengono_il_loro_permesso() {
    let mut l = lavoro_routine(Some("always"), false);
    l.richiesta.provider_id = "grok".into();
    l.richiesta.permessi = Some("always".into());
    assert!(!claude_chiede(&l.richiesta));
    let (r, _) = richiesta_turno(&Config::predefinita(), &l, None, Path::new("/x"));
    assert_eq!(r.permessi.as_deref(), Some("always"));
}

#[test]
fn allegato_scritto_0600_e_nome_ripulito() {
    let cartella = std::env::temp_dir().join(format!("plw-al-{}", &xbot_core::nuovo_id()[16..]));
    let mut conf = Config::predefinita();
    conf.cartella = cartella.clone();
    let v = serde_json::json!({
        "tipo": "lavoro", "id": "L9", "generazione": 1, "sessione_id": "S",
        "richiesta": {"provider_id": "finto", "testo": "guarda",
            "allegati": [
                {"nome": "../foto.png", "tipo_mime": "image/png", "dimensione": 4,
                 "contenuto_base64": "iVBORw=="},
                {"nome": "foto.png", "contenuto_base64": "YQ=="},
                "/non/esiste.txt",
                {"nome": "rotto", "contenuto_base64": "%%%"}
            ]}
    });
    let DalServer::Lavoro { id, richiesta, .. } = serde_json::from_value(v).unwrap() else {
        panic!("non è un lavoro")
    };
    assert!(!format!("{richiesta:?}").contains("iVBORw"));
    let l = Lavoro {
        id,
        generazione: 1,
        sessione_id: "S".into(),
        turno_id: None,
        richiesta: *richiesta,
    };
    let (r, avvisi) = richiesta_turno(&conf, &l, None, Path::new("/tmp/x.sock"));
    let dir = cartella.join("turni/L9/allegati");
    assert_eq!(
        r.allegati,
        vec![dir.join("foto.png"), dir.join("foto-2.png")]
    );
    assert_eq!(std::fs::read(&r.allegati[0]).unwrap(), b"\x89PNG");
    assert_eq!(std::fs::read(&r.allegati[1]).unwrap(), b"a");
    let modo = std::fs::metadata(&r.allegati[0])
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(modo & 0o777, 0o600);
    assert_eq!(avvisi.len(), 2, "{avvisi:?}");
    let _ = std::fs::remove_dir_all(&cartella);
}
