//! Prove dell'adattatore `cursor_agent`: argomenti, traduzione del campione vero, turni con il finto.
#![allow(clippy::unwrap_used, clippy::panic)]

use super::*;
use crate::esterni::prove_comuni::*;

fn testi(a: &[OsString]) -> Vec<String> {
    a.iter().map(|s| s.to_string_lossy().into_owned()).collect()
}

#[test]
fn argomenti_e_permessi() {
    let r = RichiestaTurno {
        sessione_esterna_id: Some("chat1".into()),
        modello: Some(MODELLO_ECONOMICO.into()),
        ..Default::default()
    };
    let a = testi(&argomenti(&r));
    assert_eq!(
        a[..5],
        [
            "-p",
            "--output-format",
            "stream-json",
            "--stream-partial-output",
            "--trust"
        ]
    );
    assert!(a.windows(2).any(|w| w == ["--resume", "chat1"]));
    assert!(a.windows(2).any(|w| w == ["--model", "grok-4.7-low"]));
    assert!(!a.iter().any(|x| x == "--force" || x == "--yolo"));
    let con = |p: &str| {
        testi(&argomenti(&RichiestaTurno {
            permessi: Some(p.into()),
            ..Default::default()
        }))
    };
    assert!(con("always").contains(&"--force".to_string()));
    assert!(con("never").windows(2).any(|w| w == ["--mode", "plan"]));
    assert!(!con("ask").contains(&"--force".to_string()));
}

#[test]
fn modelli_da_list_models() {
    let m = leggi_modelli(
        "Available models\n\nauto - Auto (current, default)\ngrok-4.7-low - Grok 4.7  Low\n\
         grok-4.7-low-fast - Grok 4.7  Low Fast\u{200b}\u{200b}\ngpt-5.2 - GPT-5.2\n",
    );
    let id: Vec<_> = m.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(id, ["grok-4.7-low", "grok-4.7-low-fast", "gpt-5.2"]);
    assert_eq!(m[0].etichetta, "Grok 4.7 Low (Cursor)");
    assert_eq!(m[1].etichetta, "Grok 4.7 Low Fast (Cursor)");
    assert_eq!(m[2].etichetta, "GPT-5.2");
    assert!(!modelli_fissi().is_empty());
}

#[test]
fn campione_vero_tradotto() {
    let mut tutti = Vec::new();
    let mut l = Lettore::nuovo();
    for riga in campione("cursor-stream-esempio.jsonl").lines() {
        let ev = l.riga(riga);
        // Il campione ha tre turni: un lettore per turno, come nel motore.
        let fine = ev.iter().any(|e| matches!(e, EventoTurno::Finito { .. }));
        tutti.extend(ev);
        if fine {
            l = Lettore::nuovo();
        }
    }
    let avviati = tutti
        .iter()
        .filter(|e| {
            matches!(
                e,
                EventoTurno::Avviato {
                    sessione_esterna_id: Some(_)
                }
            )
        })
        .count();
    assert_eq!(avviati, 3);
    // Turno 2 (a pezzi): dopo lo strumento la bolla nuova è fatta di delta, il riepilogo
    // uguale non si ripete. Turno 3 (senza pezzi): testo completo.
    assert!(testo(&tutti).contains("Non ci sono riuscito"));
    assert!(
        tutti
            .iter()
            .any(|e| matches!(e, EventoTurno::TestoDelta(d) if d == ".txt"))
    );
    assert!(tutti.iter().any(
        |e| matches!(e, EventoTurno::ChiamataStrumento { nome, input_json }
        if nome == "shell" && input_json["command"] == "ls")
    ));
    assert!(tutti.iter().any(
        |e| matches!(e, EventoTurno::EsitoStrumento { ok: true, esito_json, .. }
        if esito_json["stdout"] == "file-di-prova.txt\n")
    ));
    assert!(
        tutti
            .iter()
            .any(|e| matches!(e, EventoTurno::EsitoStrumento { ok: false, .. }))
    );
    assert!(
        tutti
            .iter()
            .any(|e| matches!(e, EventoTurno::Avviso(t) if t.contains("negato")))
    );
    assert!(
        tutti
            .iter()
            .any(|e| matches!(e, EventoTurno::Ragionamento(_)))
    );
    let finiti: Vec<_> = tutti
        .iter()
        .filter_map(|e| match e {
            EventoTurno::Finito {
                token_ingresso,
                token_uscita,
                sessione_esterna_id,
                ..
            } => Some((*token_ingresso, *token_uscita, sessione_esterna_id.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(finiti.len(), 3);
    assert_eq!(finiti[0].0, Some(23017));
    assert_eq!(finiti[0].1, Some(117));
    assert_eq!(
        finiti[0].2.as_deref(),
        Some("8a101ca5-f20b-4247-9577-f5a4e132c8e4")
    );
}

#[test]
fn delta_e_riepilogo_non_si_doppiano() {
    let mut l = Lettore::nuovo();
    let d = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"ci"}]},"timestamp_ms":1}"#;
    let d2 = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"ao"}]},"timestamp_ms":2}"#;
    let f = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"ciao"}]}}"#;
    let mut ev = l.riga(d);
    ev.extend(l.riga(d2));
    ev.extend(l.riga(f));
    assert_eq!(
        ev,
        [
            EventoTurno::TestoDelta("ci".into()),
            EventoTurno::TestoDelta("ao".into())
        ]
    );
}

fn provider(nome: &str) -> CursorAgent {
    CursorAgent::nuovo(finto("cursor-finto")).con_cartella_sessioni(cartella_prova(nome))
}

#[tokio::test]
async fn turno_con_il_finto() {
    let p = provider("cursor-ciao");
    let ev = turno(
        &p,
        RichiestaTurno {
            sessione_id: "S1".into(),
            sessione_esterna_id: Some("chat-vecchia".into()),
            sistema: Some("sei Postino".into()),
            testo: "ciao [strumento]".into(),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(
        ev[0],
        EventoTurno::Avviato {
            sessione_esterna_id: Some("chat-vecchia".into())
        }
    );
    // Il finto rovescia il testo ricevuto su stdin: c'è anche la cornice del sistema.
    let t = testo(&ev);
    assert!(t.starts_with("]otnemurts[ oaic"), "{t}");
    assert!(
        t.ends_with(&"<sistema>".chars().rev().collect::<String>()),
        "{t}"
    );
    assert!(
        ev.iter()
            .any(|e| matches!(e, EventoTurno::ChiamataStrumento { .. }))
    );
    assert!(
        matches!(ev.last(), Some(EventoTurno::Finito { sessione_esterna_id: Some(s), .. }) if s == "chat-vecchia")
    );
    assert!(
        p.cartella_di(&RichiestaTurno {
            sessione_id: "S1".into(),
            ..Default::default()
        })
        .is_dir()
    );
}

#[tokio::test]
async fn turno_fallito_con_stderr() {
    let p = provider("cursor-fallito");
    let ev = turno(
        &p,
        RichiestaTurno {
            sessione_id: "S2".into(),
            testo: "[fallisci]".into(),
            ..Default::default()
        },
    )
    .await;
    match ev.last() {
        Some(EventoTurno::Fallito { codice, testo }) => {
            assert_eq!(codice, "processo_uscito");
            assert!(testo.contains("ActionRequiredError"), "{testo}");
        }
        e => panic!("{e:?}"),
    }
}

#[tokio::test]
async fn interruzione() {
    let p = provider("cursor-lento");
    let (tx, mut rx) = mpsc::channel(64);
    let m = p
        .avvia(
            RichiestaTurno {
                sessione_id: "S3".into(),
                testo: "[lento]".into(),
                ..Default::default()
            },
            tx,
        )
        .await
        .unwrap();
    assert!(matches!(rx.recv().await, Some(EventoTurno::Avviato { .. })));
    (m.interrompi)();
    let fine = tokio::time::timeout(Duration::from_secs(10), async {
        while rx.recv().await.is_some() {}
    })
    .await;
    assert!(fine.is_ok());
}

#[tokio::test]
async fn programma_mancante() {
    let p = CursorAgent::nuovo("/non/esiste/agent")
        .con_cartella_sessioni(cartella_prova("cursor-manca"));
    let (tx, _rx) = mpsc::channel(4);
    let e = p.avvia(RichiestaTurno::default(), tx).await.unwrap_err();
    assert!(e.to_string().contains("non parte"), "{e}");
    assert_eq!(p.id(), "cursor_agent");
    assert!(p.quota().await.unwrap().is_none());
}

#[test]
fn tutti_i_modelli_anche_fast() {
    let m = leggi_modelli(
        "auto - Auto (current, default)\ngrok-4.7-low - Grok 4.7 Low\ngrok-4.7-low-fast - Grok 4.7 Low Fast\n\
         gpt-5.3-codex - Codex 5.3\nclaude-opus-5-5-max-fast - Claude Opus 5.5 1M Max Fast\ncomposer-2.5 - Composer 2.5",
    );
    let id: Vec<&str> = m.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        id,
        vec![
            "grok-4.7-low",
            "grok-4.7-low-fast",
            "gpt-5.3-codex",
            "claude-opus-5-5-max-fast",
            "composer-2.5"
        ]
    );
    assert_eq!(m[1].dettaglio.as_deref(), Some("fast"));
    assert_eq!(m[0].dettaglio, None);
}
