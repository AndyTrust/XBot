//! Prove dell'adattatore `codex`.
#![allow(clippy::unwrap_used, clippy::panic)]

use super::*;
use crate::esterni::prove_comuni::*;

fn testi(a: &[OsString]) -> Vec<String> {
    a.iter().map(|s| s.to_string_lossy().into_owned()).collect()
}

#[test]
fn argomenti_nuovo_e_ripresa() {
    let a = testi(&argomenti(&RichiestaTurno {
        modello: Some(MODELLO_ECONOMICO.into()),
        livello: Some("low".into()),
        permessi: Some("never".into()),
        ..Default::default()
    }));
    assert_eq!(a[..3], ["exec", "--json", "--skip-git-repo-check"]);
    assert!(a.windows(2).any(|w| w == ["-m", "gpt-5.6-luna"]));
    assert!(
        a.windows(2)
            .any(|w| w == ["-c", "model_reasoning_effort=\"low\""])
    );
    assert!(
        a.windows(2)
            .any(|w| w == ["-c", "sandbox_mode=\"read-only\""])
    );
    assert_eq!(a.last().unwrap(), "-");
    let a = testi(&argomenti(&RichiestaTurno {
        sessione_esterna_id: Some("T1".into()),
        livello: Some("max".into()),
        permessi: Some("always".into()),
        ..Default::default()
    }));
    assert_eq!(a[..2], ["exec", "resume"]);
    assert_eq!(a[a.len() - 2..], ["T1", "-"]);
    assert!(
        a.windows(2)
            .any(|w| w == ["-c", "sandbox_mode=\"workspace-write\""])
    );
    assert!(!a.iter().any(|x| x.contains("dangerously")));
    // Nessun limite sui livelli: passa quello che l'utente sceglie (il catalogo dice quali vanno).
    assert!(
        a.windows(2)
            .any(|w| w == ["-c", "model_reasoning_effort=\"max\""])
    );
    let a = testi(&argomenti(&RichiestaTurno {
        livello: Some("ma x\"".into()),
        ..Default::default()
    }));
    assert!(!a.iter().any(|x| x.contains("effort")));
}

#[test]
fn campione_vero_tradotto() {
    let mut tutti = Vec::new();
    let mut l = Lettore::nuovo();
    for riga in campione("codex-stream-esempio.jsonl").lines() {
        let ev = l.riga(riga);
        let fine = l.finito();
        tutti.extend(ev);
        if fine {
            l = Lettore::nuovo();
        }
    }
    let n = |f: fn(&EventoTurno) -> bool| tutti.iter().filter(|e| f(e)).count();
    assert_eq!(
        n(|e| matches!(
            e,
            EventoTurno::Avviato {
                sessione_esterna_id: Some(_)
            }
        )),
        3
    );
    assert_eq!(n(|e| matches!(e, EventoTurno::Finito { .. })), 3);
    assert_eq!(n(|e| matches!(e, EventoTurno::ChiamataStrumento { .. })), 3);
    assert!(tutti.contains(&EventoTurno::TestoCompleto("ciao".into())));
    assert!(tutti.iter().any(
        |e| matches!(e, EventoTurno::EsitoStrumento { ok: true, esito_json, .. }
        if esito_json["output"] == "file-di-prova.txt\n")
    ));
    assert!(
        tutti
            .iter()
            .any(|e| matches!(e, EventoTurno::EsitoStrumento { ok: false, .. }))
    );
    assert!(
        tutti
            .iter()
            .any(|e| matches!(e, EventoTurno::Avviso(t) if t.contains("hook")))
    );
    assert!(tutti.contains(&EventoTurno::Finito {
        token_ingresso: Some(18656),
        token_uscita: Some(6),
        token_cache: Some(6656),
        costo_usd: None,
        sessione_esterna_id: Some("f3466615-9dc8-70a3-89c1-42a9c42e593f".into()),
    }));
}

#[test]
fn due_messaggi_nella_stessa_bolla() {
    let mut l = Lettore::nuovo();
    let m = |t: &str| {
        format!(r#"{{"type":"item.completed","item":{{"type":"agent_message","text":"{t}"}}}}"#)
    };
    assert_eq!(
        l.riga(&m("uno")),
        [EventoTurno::TestoCompleto("uno".into())]
    );
    assert_eq!(
        l.riga(&m("due")),
        [EventoTurno::TestoCompleto("uno\n\ndue".into())]
    );
}

fn provider(nome: &str) -> Codex {
    Codex::nuovo(finto("codex-finto")).con_cartella_sessioni(cartella_prova(nome))
}

#[tokio::test]
async fn turno_con_il_finto() {
    let ev = turno(
        &provider("codex-ciao"),
        RichiestaTurno {
            sessione_id: "S1".into(),
            sessione_esterna_id: Some("T-vecchio".into()),
            testo: "ciao [strumento]".into(),
            ..Default::default()
        },
    )
    .await;
    assert_eq!(
        ev[0],
        EventoTurno::Avviato {
            sessione_esterna_id: Some("T-vecchio".into())
        }
    );
    assert_eq!(testo(&ev), "]otnemurts[ oaic");
    assert!(
        ev.iter()
            .any(|e| matches!(e, EventoTurno::EsitoStrumento { ok: true, .. }))
    );
    assert!(
        matches!(ev.last(), Some(EventoTurno::Finito { sessione_esterna_id: Some(s), .. }) if s == "T-vecchio")
    );
}

#[tokio::test]
async fn turno_fallito_e_crollo() {
    let ev = turno(
        &provider("codex-fallito"),
        RichiestaTurno {
            sessione_id: "S2".into(),
            testo: "[fallisci]".into(),
            ..Default::default()
        },
    )
    .await;
    assert!(
        matches!(ev.last(), Some(EventoTurno::Fallito { codice, .. }) if codice == "turno_fallito")
    );
    let ev = turno(
        &provider("codex-crolla"),
        RichiestaTurno {
            sessione_id: "S3".into(),
            testo: "[crolla]".into(),
            ..Default::default()
        },
    )
    .await;
    assert!(
        matches!(ev.last(), Some(EventoTurno::Fallito { codice, testo })
        if codice == "processo_uscito" && testo.contains("not logged in"))
    );
}

#[test]
fn catalogo_intero_anche_nascosti() {
    let m = leggi_catalogo(
        r#"{"models":[{"slug":"gpt-6-astra","display_name":"GPT-6-Astra","description":"Frontier.",
        "visibility":"list","supported_reasoning_levels":[{"effort":"low"},{"effort":"ultra"}]},
        {"slug":"gpt-reserve","display_name":"GPT-Reserve","visibility":"hide",
        "supported_reasoning_levels":[{"effort":"low"}]},
        {"slug":"gpt-5.6-luna","display_name":"GPT-5.6-Luna","visibility":"list"}]}"#,
    );
    let id: Vec<_> = m.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(id, ["gpt-6-astra", "gpt-5.6-luna", "gpt-reserve"]);
    assert_eq!(m[0].livelli, ["low", "ultra"]);
    assert_eq!(m[2].dettaglio.as_deref(), Some("nascosto in codex"));
    assert!(leggi_catalogo("non json").is_empty());
    assert_eq!(modelli_fissi().len(), 7);
}
