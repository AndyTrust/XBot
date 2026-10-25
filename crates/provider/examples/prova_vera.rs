//! Prova a mano di un adattatore con il client vero:
//! `cargo run -p xbot-provider --example prova_vera -- <cursor_agent|codex|gemini|grok> <cartella>
//!  <permesso never|ask|always> [ripresa|-] <testo>`
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use tokio::sync::mpsc;
use xbot_provider::{
    Codex, CursorAgent, Gemini, Grok, Provider, RichiestaTurno, codex, cursor_agent, gemini, grok,
};

#[tokio::main]
async fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let (client, cartella, permesso, ripresa, testo) = (&a[0], &a[1], &a[2], &a[3], &a[4]);
    let (p, modello): (Box<dyn Provider>, &str) = match client.as_str() {
        "cursor_agent" => (
            Box::new(CursorAgent::nuovo(
                cursor_agent::trova_nel_path().expect("agent"),
            )),
            cursor_agent::MODELLO_ECONOMICO,
        ),
        "codex" => (
            Box::new(Codex::nuovo(codex::trova_nel_path().expect("codex"))),
            codex::MODELLO_ECONOMICO,
        ),
        "grok" => (
            Box::new(Grok::nuovo(grok::trova_nel_path().expect("grok"))),
            grok::MODELLO_ECONOMICO,
        ),
        _ => (
            Box::new(Gemini::nuovo(gemini::trova_nel_path().expect("gemini"))),
            gemini::MODELLO_ECONOMICO,
        ),
    };
    let r = RichiestaTurno {
        sessione_id: "prova".into(),
        sessione_esterna_id: (ripresa != "-").then(|| ripresa.clone()),
        modello: Some(modello.into()),
        livello: (client == "codex").then(|| "low".into()),
        cartella_lavoro: Some(cartella.into()),
        testo: testo.clone(),
        sistema: Some("Sei un assistente di prova di XBot. Rispondi in italiano.".into()),
        permessi: Some(permesso.clone()),
        ..Default::default()
    };
    let inizio = std::time::Instant::now();
    let (tx, mut rx) = mpsc::channel(256);
    let _m = p.avvia(r, tx).await.unwrap();
    while let Some(e) = rx.recv().await {
        println!("{:.1}s {e:?}", inizio.elapsed().as_secs_f64());
    }
}
