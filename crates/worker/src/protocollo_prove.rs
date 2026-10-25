//! Prove del protocollo del canale.

use super::*;
use serde_json::json;

#[test]
fn evento_andata_e_ritorno() {
    let e = EventoTurno::ChiamataStrumento {
        nome: "Bash".into(),
        input_json: json!({"command": "ls"}),
    };
    let f = EventoFilo::from(e.clone());
    let v = serde_json::to_value(&f).unwrap();
    assert_eq!(v["tipo"], "chiamata_strumento");
    let indietro: EventoFilo = serde_json::from_value(v).unwrap();
    assert_eq!(EventoTurno::from(indietro), e);
}

#[test]
fn frame_lavoro_con_segreti() {
    let v = json!({
        "tipo": "lavoro", "id": "L1", "generazione": 3, "sessione_id": "S1",
        "turno_id": "T1", "recinto_id": "R1",
        "richiesta": {
            "provider_id": "claude_code", "testo": "ciao", "modello": "claude-haiku-5-5",
            "ambiente": [{"nome": "TOKEN_X", "valore": "segretissimo"}],
            "collegamenti": [{"nome": "odoo", "trasporto": "http", "url": "https://o/mcp",
                "intestazioni": [{"nome": "Authorization", "valore": "Bearer zz"}]}],
            "chrome": true,
            "campo_nuovo": 1
        }
    });
    let f: DalServer = serde_json::from_value(v).unwrap();
    let DalServer::Lavoro { richiesta, .. } = &f else {
        panic!("non è un lavoro")
    };
    assert!(richiesta.chrome && richiesta.chiedi_permessi);
    assert_eq!(richiesta.ambiente[0].valore.esponi(), "segretissimo");
    let debug = format!("{f:?}");
    assert!(!debug.contains("segretissimo") && !debug.contains("Bearer zz"));
}

#[test]
fn frame_sconosciuto_e_ok() {
    let f: DalServer = serde_json::from_str(r#"{"tipo":"nuovo","x":1}"#).unwrap();
    assert!(matches!(f, DalServer::Sconosciuto));
    let f: DalServer =
        serde_json::from_str(r#"{"tipo":"ok","lavoro_id":"L","fino_a_seq":4}"#).unwrap();
    assert!(matches!(f, DalServer::Ok { fino_a_seq: 4, .. }));
    let c = Corpo::Chiuso {
        stato: "riuscito".into(),
        motivo: None,
    }
    .in_frame("L", 7);
    assert_eq!(serde_json::to_value(&c).unwrap()["tipo"], "chiuso");
    assert_eq!(c.lavoro_seq(), Some(("L", 7)));
}

// --- 2.I ---
#[test]
fn account_nel_battito_e_frame_di_accesso() {
    let p = ProviderMacchina {
        id: "cursor_agent".into(),
        nome: "Cursor".into(),
        programma: "/bin/agent".into(),
        versione: None,
        modelli: vec![],
        account: Some(xbot_provider::Account {
            nome: "utente@mac.it".into(),
            dettaglio: None,
        }),
    };
    let b = DalLavoratore::Battito {
        posti_liberi: 2,
        provider: vec![p],
        lavori: vec![],
    };
    let v = serde_json::to_value(&b).unwrap();
    assert_eq!(v["provider"][0]["account"]["nome"], "utente@mac.it");
    let vecchio: ProviderMacchina =
        serde_json::from_value(json!({"id": "x", "nome": "x", "programma": "x", "modelli": []}))
            .unwrap();
    assert!(
        vecchio.account.is_none(),
        "un battito senza account si legge"
    );
    let f: DalServer = serde_json::from_value(
        json!({"tipo": "accedi", "richiesta_id": "r1", "provider_id": "codex"}),
    )
    .unwrap();
    assert!(matches!(f, DalServer::Accedi { ref provider_id, .. } if provider_id == "codex"));
    let f: DalServer = serde_json::from_value(
        json!({"tipo": "codice_accesso", "provider_id": "claude_code", "codice": "abc"}),
    )
    .unwrap();
    assert!(matches!(f, DalServer::CodiceAccesso { .. }));
    let a = DalLavoratore::Accesso {
        richiesta_id: Some("r1".into()),
        provider_id: "codex".into(),
        stato: "in_attesa".into(),
        url: Some("https://auth.openai.com/codex/device".into()),
        codice_utente: Some("ABCD-12345".into()),
        chiede_codice: false,
        account: None,
        messaggio: None,
        comando: None,
    };
    let v = serde_json::to_value(&a).unwrap();
    assert_eq!(
        (v["tipo"].as_str(), v["stato"].as_str()),
        (Some("accesso"), Some("in_attesa"))
    );
    assert_eq!(a.lavoro_seq(), None);
}
