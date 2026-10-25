//! Prove dell'adattatore `grok`: argomenti, modelli e livelli, campione vero, turni con il finto.
#![allow(clippy::unwrap_used, clippy::panic)]

use super::*;
use crate::esterni::prove_comuni::*;

fn testi(a: &[OsString]) -> Vec<String> {
    a.iter().map(|s| s.to_string_lossy().into_owned()).collect()
}

fn provider(nome: &str) -> Grok {
    Grok::nuovo(finto("grok-finto"))
        .con_cartella_sessioni(cartella_prova(nome))
        .con_casa(cartella_prova(nome).join("grok-home"))
}

#[test]
fn argomenti_e_permessi() {
    let r = RichiestaTurno {
        testo: "-ciao".into(),
        modello: Some(MODELLO_ECONOMICO.into()),
        livello: Some("high".into()),
        sistema: Some("sei Postino".into()),
        ..Default::default()
    };
    let a = testi(&argomenti(&r, "U1"));
    assert_eq!(
        a[..3],
        ["--single=-ciao", "--output-format", "streaming-json"]
    );
    assert!(a.contains(&"--session-id=U1".to_string()));
    assert!(a.windows(2).any(|w| w == ["-m", "grok-4.5"]));
    assert!(a.windows(2).any(|w| w == ["--reasoning-effort", "high"]));
    assert!(a.contains(&"--rules=sei Postino".to_string()));
    assert!(a.windows(2).any(|w| w == ["--permission-mode", "dontAsk"]));
    assert!(!a.contains(&"--always-approve".to_string()));
    let con = |p: &str| {
        testi(&argomenti(
            &RichiestaTurno {
                sessione_esterna_id: Some("S9".into()),
                permessi: Some(p.into()),
                livello: Some("a b".into()),
                ..Default::default()
            },
            "U2",
        ))
    };
    let sempre = con("always");
    assert!(sempre.contains(&"--always-approve".to_string()));
    assert!(sempre.contains(&"--resume=S9".to_string()));
    assert!(!sempre.iter().any(|x| x.starts_with("--session-id")));
    assert!(!sempre.iter().any(|x| x == "--reasoning-effort"));
    let mai = con("never");
    for n in NEGATI_SE_MAI {
        assert!(mai.contains(&format!("--deny={n}")), "{mai:?}");
    }
}

#[test]
fn modelli_e_livelli() {
    let m = leggi_modelli(
        "You are logged in with grok.com.\n\nDefault model: grok-4.7\n\nAvailable models:\n  \
         * grok-4.7 (default)\n  - grok-4.7-build-fast\n  - grok-4.6\n  - grok-4.5\n",
    );
    let id: Vec<_> = m.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        id,
        ["grok-4.7", "grok-4.7-build-fast", "grok-4.6", "grok-4.5"]
    );
    assert_eq!(m[0].etichetta, "Grok 4.7");
    assert_eq!(m[0].dettaglio.as_deref(), Some("predefinito"));
    assert_eq!(m[1].dettaglio.as_deref(), Some("fast"));
    assert_eq!(
        leggi_livelli(
            "{\"type\":\"error\",\"message\":\"--effort/--reasoning-effort: unknown effort level \
             'xbot-sonda'; use one of: xhigh, high, medium, low\",\"cwd\":\"/tmp\"}"
        ),
        ["low", "medium", "high", "xhigh"]
    );
    assert!(leggi_livelli("altro").is_empty());
    assert_eq!(uuid_nuovo().len(), 36);
    assert_ne!(uuid_nuovo(), uuid_nuovo());
}

#[tokio::test]
async fn modelli_dal_finto_con_la_sonda() {
    let m = provider("grok-modelli").modelli().await.unwrap();
    assert_eq!(m.len(), 4);
    assert_eq!(m[0].livelli, ["low", "medium", "high", "xhigh"]);
    assert_eq!(m[3].id, "grok-4.5");
    assert_eq!(m[3].livelli, ["low", "medium", "high"]);
}

#[test]
fn campione_vero_tradotto() {
    let mut tutti = Vec::new();
    let mut l = Lettore::nuovo(Some("S".into()));
    for riga in campione("grok-stream-esempio.jsonl").lines() {
        let ev = l.riga(riga);
        let fine = l.finito();
        tutti.extend(ev);
        if fine {
            l = Lettore::nuovo(Some("S".into()));
        }
    }
    let n = |f: fn(&EventoTurno) -> bool| tutti.iter().filter(|e| f(e)).count();
    // Cinque turni: ciao, ls, comando negato, ripresa, scrittura con --always-approve.
    assert_eq!(n(|e| matches!(e, EventoTurno::Avviato { .. })), 5);
    assert_eq!(n(|e| matches!(e, EventoTurno::Finito { .. })), 5);
    assert!(n(|e| matches!(e, EventoTurno::Ragionamento(_))) > 0);
    assert!(tutti.iter().any(
        |e| matches!(e, EventoTurno::ChiamataStrumento { nome, input_json }
        if nome == "run_terminal_command" && input_json["command"] == "ls -la")
    ));
    assert!(tutti.iter().any(
        |e| matches!(e, EventoTurno::EsitoStrumento { ok: true, esito_json, .. }
        if esito_json["command"] == "ls -la")
    ));
    assert!(tutti.iter().any(
        |e| matches!(e, EventoTurno::Avviso(t) if t.contains("negato") && t.contains("echo ciao"))
    ));
    assert_eq!(
        n(|e| matches!(e, EventoTurno::EsitoStrumento { ok: false, .. })),
        1
    );
    let sessioni: Vec<_> = tutti
        .iter()
        .filter_map(|e| match e {
            EventoTurno::Finito {
                sessione_esterna_id: Some(s),
                token_uscita: Some(_),
                costo_usd: Some(_),
                ..
            } => Some(s.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(sessioni.len(), 5);
    // La ripresa (turno 4) ha lo stesso id del primo.
    assert_eq!(sessioni[0], sessioni[3]);
    assert!(!testo(&tutti).is_empty());
}

#[tokio::test]
async fn turno_con_il_finto() {
    let ev = turno(
        &provider("grok-turno"),
        RichiestaTurno {
            sessione_id: "S1".into(),
            testo: "ciao [strumento]".into(),
            ..Default::default()
        },
    )
    .await;
    let Some(EventoTurno::Avviato {
        sessione_esterna_id: Some(sid),
    }) = ev.first()
    else {
        panic!("{ev:?}")
    };
    assert_eq!(sid.len(), 36);
    assert_eq!(testo(&ev), "]otnemurts[ oaic");
    assert!(
        ev.iter()
            .any(|e| matches!(e, EventoTurno::EsitoStrumento { ok: true, .. }))
    );
    assert!(
        matches!(ev.last(), Some(EventoTurno::Finito { sessione_esterna_id: Some(s), .. }) if s == sid)
    );
    let ev = turno(
        &provider("grok-ripresa"),
        RichiestaTurno {
            sessione_id: "S1".into(),
            sessione_esterna_id: Some("vecchia".into()),
            testo: "[negato]".into(),
            ..Default::default()
        },
    )
    .await;
    assert!(
        ev.iter()
            .any(|e| matches!(e, EventoTurno::Avviso(t) if t.contains("negato")))
    );
    assert!(
        matches!(ev.last(), Some(EventoTurno::Finito { sessione_esterna_id: Some(s), .. }) if s == "vecchia")
    );
}

#[tokio::test]
async fn non_autenticato_e_programma_mancante() {
    let ev = turno(
        &provider("grok-fallito"),
        RichiestaTurno {
            sessione_id: "S2".into(),
            testo: "[fallisci]".into(),
            ..Default::default()
        },
    )
    .await;
    assert!(
        matches!(ev.last(), Some(EventoTurno::Fallito { codice, testo })
        if codice == "non_autenticato" && testo.contains("grok login"))
    );
    let p = Grok::nuovo("/non/esiste/grok")
        .con_cartella_sessioni(cartella_prova("grok-manca"))
        .con_casa(cartella_prova("grok-manca").join("grok-home"));
    let (tx, _rx) = mpsc::channel(4);
    let e = p.avvia(RichiestaTurno::default(), tx).await.unwrap_err();
    assert!(e.to_string().contains("non parte"), "{e}");
    assert_eq!(p.id(), "grok");
    assert_eq!(p.nome(), "Grok (xAI)");
    assert!(p.quota().await.unwrap().is_none());
}

struct Approva(std::sync::Mutex<Vec<String>>);

#[async_trait::async_trait]
impl crate::ChiediPermesso for Approva {
    async fn chiedi(&self, strumento: &str, input: &serde_json::Value) -> Result<(), String> {
        let c = input["command"].as_str().unwrap_or_default();
        if let Ok(mut v) = self.0.lock() {
            v.push(format!("{strumento}: {c}"));
        }
        Ok(())
    }
}

/// Fase 2.G: due turni sullo stesso `grok agent stdio`; il permesso `ask` passa da chi decide;
/// un cambio di livello riapre il processo.
#[tokio::test]
async fn processo_caldo_acp() {
    let p = provider("grok-caldo");
    let base = RichiestaTurno {
        sessione_id: "S-caldo".into(),
        livello: Some("low".into()),
        testo: "chi [pid]".into(),
        ..Default::default()
    };
    let t1 = testo(&turno(&p, base.clone()).await);
    let t2 = testo(&turno(&p, base.clone()).await);
    let pid = |t: &str| t.split_whitespace().nth(1).unwrap_or_default().to_string();
    assert_eq!(pid(&t1), pid(&t2), "{t1} / {t2}");
    assert!(t2.contains("turno 2 livello low"), "{t2}");
    let chi = std::sync::Arc::new(Approva(std::sync::Mutex::default()));
    let ev = turno(
        &p,
        RichiestaTurno {
            testo: "[negato]".into(),
            chiedi_permesso: Some(crate::Permessi(chi.clone())),
            ..base.clone()
        },
    )
    .await;
    assert_eq!(
        chi.0.lock().unwrap().as_slice(),
        ["Bash: echo x > prova4.txt"]
    );
    assert!(
        ev.iter()
            .any(|e| matches!(e, EventoTurno::EsitoStrumento { ok: true, .. }))
    );
    assert!(
        matches!(ev.last(), Some(EventoTurno::Finito { .. })),
        "{ev:?}"
    );
    let t3 = testo(
        &turno(
            &p,
            RichiestaTurno {
                livello: Some("high".into()),
                ..base.clone()
            },
        )
        .await,
    );
    assert_ne!(pid(&t1), pid(&t3), "{t3}");
    assert!(t3.contains("turno 1 livello high"), "{t3}");
}

/// Fase 2.G-bis: grok parte nella casa isolata, con le variabili della casa in fondo
/// (vincono su quelle della richiesta).
#[tokio::test]
async fn casa_isolata() {
    let p = provider("grok-casa");
    let casa = cartella_prova("grok-casa").join("grok-home");
    let r = RichiestaTurno {
        sessione_id: "S-casa".into(),
        testo: "[ambiente]".into(),
        ambiente: vec![("HOME".into(), xbot_core::SegretoTesto::nuovo("/altrove"))],
        ..Default::default()
    };
    let t = testo(&turno(&p, r).await);
    let v: serde_json::Value = serde_json::from_str(&t).unwrap_or_else(|_| panic!("{t}"));
    assert_eq!(v["HOME"], casa.to_string_lossy().as_ref());
    assert_eq!(v["GROK_HOME"], casa.to_string_lossy().as_ref());
    assert_eq!(v["GROK_MEMORY"], "0");
    assert_eq!(v["GROK_SUBAGENTS"], "0");
    assert_eq!(v["GROK_TELEMETRY_ENABLED"], "0");
    assert!(casa.join("config.toml").exists());
    assert!(casa.join("requirements.toml").exists());
}

/// §22: niente `XBOT_*` del server né chiavi ereditate; `DISPLAY` e `XBOT_COMPUTER_HOME` del
/// turno arrivano; un `DISPLAY` nuovo cambia la firma e apre un processo caldo nuovo.
#[tokio::test]
async fn ambiente_del_turno() {
    let p = provider("grok-ambiente");
    let base = RichiestaTurno {
        sessione_id: "S-ambiente".into(),
        testo: "[ambiente:XBOT_CASSAFORTE_CHIAVE] [ambiente:XBOT_PROVA_OPENAI_API_KEY] \
                [ambiente:DISPLAY] [ambiente:XBOT_COMPUTER_HOME]"
            .into(),
        ..Default::default()
    };
    let t = testo(&turno(&p, base.clone()).await);
    assert_eq!(
        t,
        "XBOT_CASSAFORTE_CHIAVE assente XBOT_PROVA_OPENAI_API_KEY assente DISPLAY assente \
         XBOT_COMPUTER_HOME assente"
    );
    let pid = |t: &str| t.split_whitespace().nth(1).unwrap_or_default().to_string();
    let chi = RichiestaTurno {
        testo: "chi [pid]".into(),
        ..base.clone()
    };
    let prima = pid(&testo(&turno(&p, chi.clone()).await));
    let col_computer = |r: RichiestaTurno| RichiestaTurno {
        ambiente: vec![
            ("DISPLAY".into(), xbot_core::SegretoTesto::nuovo(":50")),
            (
                "XBOT_COMPUTER_HOME".into(),
                xbot_core::SegretoTesto::nuovo("/tmp/computer"),
            ),
        ],
        ..r
    };
    let dopo = pid(&testo(&turno(&p, col_computer(chi)).await));
    assert_ne!(prima, dopo, "DISPLAY nuovo: processo caldo nuovo");
    let t = testo(&turno(&p, col_computer(base)).await);
    assert_eq!(
        t,
        "XBOT_CASSAFORTE_CHIAVE assente XBOT_PROVA_OPENAI_API_KEY assente DISPLAY presente \
         XBOT_COMPUTER_HOME presente"
    );
}
