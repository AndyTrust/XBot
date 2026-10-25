//! Misura della Fase 2.G con i client veri: tempo al primo testo del secondo turno di una
//! sessione. `XBOT_CALDO_MINUTI=0` = freddo (un processo per turno, `--resume`), altrimenti caldo.
//!
//! `cargo run -p xbot-provider --example misura_caldi -- claude|grok [giri]`
//! `XBOT_MISURA_CARTELLA=<cartella>` lavora lì (una cartella vera, non temporanea: grok spegne
//! memoria e indice nelle cartelle temporanee e la misura sembrerebbe migliore del vero).

use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use xbot_provider::{ClaudeCode, EventoTurno, Grok, Provider, RichiestaTurno};

/// (primo testo, fine del turno, id della sessione esterna, testo).
async fn turno(
    p: &dyn Provider,
    r: RichiestaTurno,
) -> (Option<Duration>, Option<Duration>, Option<String>, String) {
    let (tx, mut rx) = mpsc::channel(256);
    let t = Instant::now();
    let _m = p.avvia(r, tx).await.unwrap_or_else(|e| panic!("{e}"));
    let (mut primo, mut fine, mut sid, mut testo) = (None, None, None, String::new());
    while let Ok(Some(e)) = tokio::time::timeout(Duration::from_secs(120), rx.recv()).await {
        match e {
            EventoTurno::TestoDelta(d) => {
                primo.get_or_insert_with(|| t.elapsed());
                testo.push_str(&d);
            }
            EventoTurno::Finito {
                sessione_esterna_id,
                ..
            } => {
                fine = Some(t.elapsed());
                sid = sessione_esterna_id;
            }
            EventoTurno::Fallito { testo: f, .. } => panic!("turno fallito: {f}"),
            _ => {}
        }
    }
    (primo, fine, sid, testo)
}

#[tokio::main]
async fn main() {
    let quale = std::env::args().nth(1).unwrap_or_else(|| "claude".into());
    let giri: usize = std::env::args()
        .nth(2)
        .and_then(|g| g.parse().ok())
        .unwrap_or(3);
    let cartella = std::env::temp_dir().join("xbot-misura-2g");
    let (p, modello): (Box<dyn Provider>, &str) = match quale.as_str() {
        "grok" => (
            Box::new(
                Grok::nuovo(xbot_provider::grok::trova_nel_path().unwrap())
                    .con_cartella_sessioni(&cartella),
            ),
            "grok-4.7",
        ),
        _ => (
            Box::new(
                ClaudeCode::nuovo(xbot_provider::claude_code::trova_nel_path().unwrap())
                    .con_cartella_sessioni(&cartella),
            ),
            "claude-sonnet-5-5",
        ),
    };
    let modo = if xbot_provider::caldi::accesi() {
        "caldo"
    } else {
        "freddo"
    };
    for g in 0..giri {
        let mut r = RichiestaTurno {
            sessione_id: format!("misura-{}-{g}", xbot_core::nuovo_id()),
            modello: Some(modello.into()),
            livello: Some("low".into()),
            permessi: Some("never".into()),
            testo: "Rispondi solo con la parola: ciao".into(),
            cartella_lavoro: std::env::var_os("XBOT_MISURA_CARTELLA").map(Into::into),
            ..Default::default()
        };
        let (p1, f1, sid, _) = turno(p.as_ref(), r.clone()).await;
        r.sessione_esterna_id = sid;
        let (p2, f2, _, t2) = turno(p.as_ref(), r).await;
        let s = |d: Option<Duration>| d.map_or(f64::NAN, |d| d.as_secs_f64());
        println!(
            "{quale} {modo} giro {g}: primo turno testo {:.2} s fine {:.2} s, secondo turno testo {:.2} s fine {:.2} s («{}»)",
            s(p1),
            s(f1),
            s(p2),
            s(f2),
            t2.trim()
        );
    }
    xbot_provider::caldi::chiudi_tutti();
}
