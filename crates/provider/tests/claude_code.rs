//! L'adattatore `claude_code` contro il `claude` finto (`prove/claude-finto`).

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use xbot_provider::{ClaudeCode, EventoTurno, ManigliaTurno, Provider, RichiestaTurno};

fn finto() -> PathBuf {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("prove/claude-finto");
    // OneDrive a volte perde il bit di esecuzione.
    let mut perm = std::fs::metadata(&p).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&p, perm).unwrap();
    p
}

fn adattatore() -> ClaudeCode {
    let sessioni = std::env::temp_dir().join(format!("xbot-cc-{}", xbot_core::nuovo_id()));
    ClaudeCode::nuovo(finto()).con_cartella_sessioni(sessioni)
}

fn richiesta(testo: &str) -> RichiestaTurno {
    RichiestaTurno {
        sessione_id: xbot_core::nuovo_id(),
        modello: Some("claude-haiku-5-5".into()),
        testo: testo.into(),
        ..Default::default()
    }
}

async fn avvia(c: &ClaudeCode, r: RichiestaTurno) -> (ManigliaTurno, mpsc::Receiver<EventoTurno>) {
    let (tx, rx) = mpsc::channel(64);
    let m = c.avvia(r, tx).await.unwrap();
    (m, rx)
}

async fn raccogli(c: &ClaudeCode, r: RichiestaTurno) -> Vec<EventoTurno> {
    let (_m, mut rx) = avvia(c, r).await;
    let mut v = Vec::new();
    while let Some(e) = tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .unwrap()
    {
        v.push(e);
    }
    v
}

fn vivo(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[tokio::test]
async fn turno_completo_con_quota_e_token() {
    let c = adattatore();
    let r = richiesta("ciao Mario");
    let cartella = c.cartella_di(&r);
    let v = raccogli(&c, r).await;
    let EventoTurno::Avviato {
        sessione_esterna_id: Some(sid),
    } = &v[0]
    else {
        panic!("prima riga: {:?}", v[0]);
    };
    assert!(cartella.is_dir(), "la cartella della sessione è creata");
    let delta: Vec<String> = v
        .iter()
        .filter_map(|e| match e {
            EventoTurno::TestoDelta(t) => Some(t.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(delta.len(), 3);
    assert_eq!(delta.concat(), "oiraM oaic");
    assert!(v.contains(&EventoTurno::TestoCompleto("oiraM oaic".into())));
    assert!(v.contains(&EventoTurno::Quota {
        finestra: "seven_day".into(),
        utilizzo: 0.24,
        azzera_ms: Some(1_793_124_000_000)
    }));
    assert_eq!(
        v.last(),
        Some(&EventoTurno::Finito {
            token_ingresso: Some(51_939),
            token_uscita: Some(5),
            token_cache: Some(23_919),
            costo_usd: Some(0.005_845_49),
            sessione_esterna_id: Some(sid.clone()),
        })
    );
}

#[tokio::test]
async fn resume_strumento_e_fallimento() {
    let c = adattatore();
    let mut r = richiesta("vai [strumento]");
    r.sessione_esterna_id = Some("sessione-di-prima".into());
    let v = raccogli(&c, r).await;
    assert_eq!(
        v[0],
        EventoTurno::Avviato {
            sessione_esterna_id: Some("sessione-di-prima".into())
        }
    );
    assert!(v.contains(&EventoTurno::ChiamataStrumento {
        nome: "Bash".into(),
        input_json: serde_json::json!({ "command": "echo ciao", "description": "prova" })
    }));
    assert!(v.contains(&EventoTurno::EsitoStrumento {
        nome: "Bash".into(),
        esito_json: serde_json::json!("ciao"),
        ok: true
    }));

    let v = raccogli(&c, richiesta("prova [fallisci]")).await;
    assert_eq!(
        v.last(),
        Some(&EventoTurno::Fallito {
            codice: "error_during_execution".into(),
            testo: "errore finto su richiesta".into()
        })
    );

    let v = raccogli(&c, richiesta("prova [crolla]")).await;
    let Some(EventoTurno::Fallito { codice, testo }) = v.last() else {
        panic!("{v:?}");
    };
    assert_eq!(codice, "processo_uscito");
    assert!(testo.contains("credenziali mancanti"), "{testo}");
}

#[tokio::test]
async fn interruzione_ferma_il_processo() {
    let c = adattatore();
    let (m, mut rx) = avvia(&c, richiesta("aspetta [lento]")).await;
    let pid = m.pid.unwrap();
    assert!(matches!(rx.recv().await, Some(EventoTurno::Avviato { .. })));
    let t = Instant::now();
    (m.interrompi)();
    assert_eq!(rx.recv().await, None);
    for _ in 0..100 {
        if !vivo(pid) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!vivo(pid), "il processo è ancora vivo");
    assert!(t.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn silenzio_interrompe() {
    let c = adattatore().con_silenzio(Duration::from_millis(500));
    let v = raccogli(&c, richiesta("zitto [muto]")).await;
    assert!(matches!(
        v.last(),
        Some(EventoTurno::Fallito { codice, .. }) if codice == "silenzio"
    ));
}

#[tokio::test]
async fn programma_che_non_esiste() {
    let c = ClaudeCode::nuovo("/non/esiste/claude")
        .con_cartella_sessioni(std::env::temp_dir().join("xbot-cc-nessuno"));
    let (tx, _rx) = mpsc::channel(4);
    let e = c.avvia(richiesta("x"), tx).await.unwrap_err();
    assert_eq!(e.codice, xbot_core::Codice::ProviderFermo);
    assert_eq!(c.modelli().await.unwrap().len(), 3);
    assert_eq!(c.quota().await.unwrap(), None);
}

fn testo_finale(v: &[EventoTurno]) -> String {
    v.iter()
        .rev()
        .find_map(|e| match e {
            EventoTurno::TestoCompleto(t) => Some(t.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn costo(v: &[EventoTurno]) -> Option<f64> {
    v.iter().find_map(|e| match e {
        EventoTurno::Finito { costo_usd, .. } => *costo_usd,
        _ => None,
    })
}

/// Fase 2.G: due turni della stessa sessione vanno sullo stesso processo; un cambio di modello
/// apre un processo nuovo; il costo è quello del turno, non il cumulativo del processo.
#[tokio::test]
async fn processo_caldo_fra_i_turni() {
    let c = adattatore();
    let r = richiesta("chi sei [pid]");
    let sid = r.sessione_id.clone();
    let v1 = raccogli(&c, r.clone()).await;
    let t1 = testo_finale(&v1);
    assert!(t1.ends_with("turno 1"), "{t1}");
    let mut r2 = r.clone();
    r2.sessione_esterna_id = Some("gia-vista".into());
    let v2 = raccogli(&c, r2.clone()).await;
    let t2 = testo_finale(&v2);
    let pid = |t: &str| t.split_whitespace().nth(1).unwrap_or_default().to_string();
    assert_eq!(pid(&t1), pid(&t2), "stesso processo: {t1} / {t2}");
    assert!(t2.ends_with("turno 2"), "{t2}");
    assert_eq!(costo(&v1), costo(&v2), "costo per turno");
    let mut r3 = r2.clone();
    r3.modello = Some("claude-sonnet-5-5".into());
    let t3 = testo_finale(&raccogli(&c, r3).await);
    assert_ne!(pid(&t1), pid(&t3), "il cambio di modello riapre: {t3}");
    assert!(t3.ends_with("turno 1"), "{t3}");
    // Un processo che crolla esce dalla cassetta: il turno dopo ne apre uno nuovo senza errore.
    let mut r4 = r.clone();
    r4.modello = Some("claude-sonnet-5-5".into());
    r4.testo = "giù [crolla]".into();
    let v4 = raccogli(&c, r4.clone()).await;
    assert!(matches!(v4.last(), Some(EventoTurno::Fallito { .. })));
    r4.testo = "di nuovo [pid]".into();
    let t5 = testo_finale(&raccogli(&c, r4).await);
    assert!(t5.ends_with("turno 1"), "{t5}");
    assert_eq!(r.sessione_id, sid);
}

/// §22: il processo `claude` non eredita le `XBOT_*` del server (cassaforte) né le chiavi;
/// `DISPLAY` e `XBOT_COMPUTER_HOME` del turno arrivano.
#[tokio::test]
async fn ambiente_pulito() {
    let c = adattatore();
    let mut r = richiesta(
        "[ambiente:XBOT_CASSAFORTE_CHIAVE] [ambiente:XBOT_PROVA_CC_API_KEY] \
         [ambiente:DISPLAY] [ambiente:XBOT_COMPUTER_HOME]",
    );
    r.ambiente = vec![
        ("DISPLAY".into(), xbot_core::SegretoTesto::nuovo(":50")),
        (
            "XBOT_COMPUTER_HOME".into(),
            xbot_core::SegretoTesto::nuovo("/tmp/computer"),
        ),
    ];
    let ev = raccogli(&c, r).await;
    let t: String = ev
        .iter()
        .filter_map(|e| match e {
            EventoTurno::TestoDelta(d) => Some(d.as_str()),
            _ => None,
        })
        .collect();
    assert!(t.contains("XBOT_CASSAFORTE_CHIAVE assente"), "{t}");
    assert!(t.contains("XBOT_PROVA_CC_API_KEY assente"), "{t}");
    assert!(t.contains("DISPLAY presente"), "{t}");
    assert!(t.contains("XBOT_COMPUTER_HOME presente"), "{t}");
}
