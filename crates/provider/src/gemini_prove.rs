//! Prove dell'adattatore `gemini` (campione scritto dal sorgente di gemini-cli 0.31.0).
#![allow(clippy::unwrap_used, clippy::panic)]

use super::*;
use crate::esterni::prove_comuni::*;

fn testi(a: &[OsString]) -> Vec<String> {
    a.iter().map(|s| s.to_string_lossy().into_owned()).collect()
}

#[test]
fn argomenti_e_permessi() {
    let a = testi(&argomenti(&RichiestaTurno {
        sessione_esterna_id: Some("S-uuid".into()),
        modello: Some(MODELLO_ECONOMICO.into()),
        testo: "-v".into(),
        ..Default::default()
    }));
    assert!(a.windows(2).any(|w| w == ["-r", "S-uuid"]));
    assert!(a.windows(2).any(|w| w == ["-m", "gemini-2.5-flash-lite"]));
    assert!(a.contains(&"--approval-mode=auto_edit".to_string()));
    assert_eq!(a.last().unwrap(), "--prompt=-v");
    let modo = |p: &str| {
        testi(&argomenti(&RichiestaTurno {
            permessi: Some(p.into()),
            ..Default::default()
        }))
    };
    assert!(modo("never").contains(&"--approval-mode=default".to_string()));
    assert!(modo("always").contains(&"--approval-mode=yolo".to_string()));
}

#[test]
fn campione_tradotto() {
    let mut l = Lettore::nuovo();
    let ev: Vec<_> = campione("gemini-stream-esempio.jsonl")
        .lines()
        .flat_map(|r| l.riga(r))
        .collect();
    assert!(l.finito());
    assert_eq!(
        ev,
        [
            EventoTurno::Avviato {
                sessione_esterna_id: Some("5f0c3c6e-0000-4000-8000-000000000001".into())
            },
            EventoTurno::TestoDelta("ciao".into()),
            EventoTurno::ChiamataStrumento {
                nome: "run_shell_command".into(),
                input_json: serde_json::json!({"command": "ls", "description": "Elenca i file"}),
            },
            EventoTurno::EsitoStrumento {
                nome: "run_shell_command".into(),
                esito_json: serde_json::json!({"output": "file-di-prova.txt", "error": null}),
                ok: true,
            },
            EventoTurno::Finito {
                token_ingresso: Some(100),
                token_uscita: Some(20),
                token_cache: Some(0),
                costo_usd: None,
                sessione_esterna_id: Some("5f0c3c6e-0000-4000-8000-000000000001".into()),
            },
        ]
    );
}

fn provider(nome: &str) -> Gemini {
    Gemini::nuovo(finto("gemini-finto")).con_cartella_sessioni(cartella_prova(nome))
}

#[tokio::test]
async fn turno_con_il_finto() {
    let ev = turno(
        &provider("gemini-ciao"),
        RichiestaTurno {
            sessione_id: "S1".into(),
            sessione_esterna_id: Some("S-vecchia".into()),
            testo: "ciao".into(),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(testo(&ev), "oaic");
    assert!(
        matches!(ev.last(), Some(EventoTurno::Finito { sessione_esterna_id: Some(s), .. }) if s == "S-vecchia")
    );
}

#[tokio::test]
async fn turno_fallito() {
    let ev = turno(
        &provider("gemini-fallito"),
        RichiestaTurno {
            sessione_id: "S2".into(),
            testo: "[fallisci]".into(),
            ..Default::default()
        },
    )
    .await;
    assert!(
        matches!(ev.last(), Some(EventoTurno::Fallito { codice, testo })
        if codice == "FatalError" && testo.contains("finto"))
    );
}
