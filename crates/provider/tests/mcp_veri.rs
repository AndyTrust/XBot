//! Fase 2.L, solo a mano sul Mac (client veri, abbonamento dell'utente, nessun accesso toccato):
//! Grok e Cursor ricevono il server MCP finto delle prove (`crates/server/tests/mcp-finto.py`)
//! dalla `ConfigMcp` del turno e usano il suo strumento `cerca`.
//! `cargo test -p xbot-provider --test mcp_veri -- --ignored --nocapture --test-threads=1`

use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::mpsc;
use xbot_core::SegretoTesto;
use xbot_provider::claude_code::mcp::{ConfigMcp, ServerMcp};
use xbot_provider::{CursorAgent, EventoTurno, Grok, Provider, RichiestaTurno, cursor_agent, grok};

const DOMANDA: &str = "Chiama lo strumento `cerca` del server MCP `finto` (nessun argomento) \
    e rispondi solo con la sua descrizione, parola per parola. Non fare altro.";

fn richiesta(modello: Option<&str>) -> RichiestaTurno {
    let finto = format!(
        "{}/../server/tests/mcp-finto.py",
        env!("CARGO_MANIFEST_DIR")
    );
    RichiestaTurno {
        sessione_id: xbot_core::nuovo_id(),
        modello: modello.map(str::to_string),
        testo: DOMANDA.into(),
        permessi: Some("always".into()),
        mcp: Some(ConfigMcp {
            server: vec![ServerMcp::Stdio {
                nome: "finto".into(),
                comando: "python3".into(),
                argomenti: vec![finto],
                ambiente: vec![("TOKEN_FINTO".into(), SegretoTesto::nuovo("xyz-segreto"))],
            }],
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn cartella(nome: &str) -> PathBuf {
    std::env::temp_dir().join(format!("xbot-mcp-veri-{nome}-{}", xbot_core::nuovo_id()))
}

/// (strumenti chiamati, testo) del turno.
async fn turno(p: &dyn Provider, r: RichiestaTurno) -> (Vec<String>, String) {
    let (tx, mut rx) = mpsc::channel(256);
    let _m = p.avvia(r, tx).await.unwrap();
    let (mut strumenti, mut testo) = (Vec::new(), String::new());
    let fine = tokio::time::Instant::now() + Duration::from_secs(240);
    while let Ok(Some(e)) = tokio::time::timeout_at(fine, rx.recv()).await {
        match e {
            EventoTurno::ChiamataStrumento { nome, .. } => strumenti.push(nome),
            EventoTurno::TestoDelta(t) => testo.push_str(&t),
            EventoTurno::TestoCompleto(t) => testo = t,
            EventoTurno::Fallito { codice, testo } => panic!("{codice}: {testo}"),
            EventoTurno::Finito { .. } => break,
            altro => println!("evento: {altro:?}"),
        }
    }
    (strumenti, testo)
}

#[tokio::test]
#[ignore]
async fn grok_usa_il_collegamento() {
    let g = Grok::nuovo(grok::trova_nel_path().unwrap())
        .con_casa(cartella("grok-casa"))
        .con_cartella_sessioni(cartella("grok-sessioni"));
    let (strumenti, testo) = turno(&g, richiesta(None)).await;
    println!("grok: strumenti {strumenti:?}, testo «{testo}»");
    // Grok passa dal suo meta-strumento `use_tool` (server_name `finto`, tool_name `cerca`).
    assert!(
        strumenti
            .iter()
            .any(|s| s.contains("cerca") || s == "use_tool"),
        "{strumenti:?}"
    );
    assert!(testo.contains("token=xyz"), "{testo}");
}

#[tokio::test]
#[ignore]
async fn cursor_usa_il_collegamento() {
    let c = CursorAgent::nuovo(cursor_agent::trova_nel_path().unwrap())
        .con_cartella_sessioni(cartella("cursor-sessioni"));
    let r = richiesta(None);
    let dove = c.cartella_di(&r);
    let (strumenti, testo) = turno(&c, r).await;
    println!("cursor: strumenti {strumenti:?}, testo «{testo}»");
    assert!(dove.join(".cursor/mcp.json").is_file());
    // Cursor chiama lo strumento come `mcp` (dopo `getMcpTools`).
    assert!(strumenti.iter().any(|s| s == "mcp"), "{strumenti:?}");
    assert!(testo.contains("token=xyz"), "{testo}");
}

/// 2.L-bis: Grok in ACP (processo caldo) con un server MCP lento 5 s: il primo turno aspetta che
/// il server sia pronto e usa lo strumento.
#[tokio::test]
#[ignore]
async fn grok_caldo_aspetta_il_server_lento() {
    assert!(
        xbot_provider::caldi::accesi(),
        "serve il modo caldo (XBOT_CALDO_MINUTI ≠ 0)"
    );
    let g = Grok::nuovo(grok::trova_nel_path().unwrap())
        .con_casa(cartella("grok-casa-lenta"))
        .con_cartella_sessioni(cartella("grok-sessioni-lente"));
    let lento = format!("{}/prove/mcp-lento.py", env!("CARGO_MANIFEST_DIR"));
    let mut r = richiesta(None);
    r.testo = "Usa lo strumento `caselle` del collegamento `posta_imap` ed elenca solo i nomi \
               delle caselle che restituisce, separati da virgole. Non fare altro."
        .into();
    // Il nome vero del catalogo: dava `Posta_IMAP_` e `Posta_IMAP___caselle`, rifiutato da grok.
    r.mcp = Some(ConfigMcp {
        server: vec![ServerMcp::Stdio {
            nome: "Posta (IMAP)".into(),
            comando: "python3".into(),
            argomenti: vec![lento],
            ambiente: vec![("LENTO_S".into(), SegretoTesto::nuovo("5"))],
        }],
        ..Default::default()
    });
    let inizio = std::time::Instant::now();
    let (tx, mut rx) = mpsc::channel(256);
    let _m = g.avvia(r, tx).await.unwrap();
    let (mut esiti, mut testo) = (Vec::new(), String::new());
    let fine = tokio::time::Instant::now() + Duration::from_secs(240);
    while let Ok(Some(e)) = tokio::time::timeout_at(fine, rx.recv()).await {
        match e {
            EventoTurno::EsitoStrumento {
                nome, esito_json, ..
            } => esiti.push((nome, esito_json)),
            EventoTurno::TestoDelta(t) => testo.push_str(&t),
            EventoTurno::Fallito { codice, testo } => panic!("{codice}: {testo}"),
            EventoTurno::Finito { .. } => break,
            _ => {}
        }
    }
    println!(
        "grok caldo in {:?}: {esiti:?}, testo «{testo}»",
        inizio.elapsed()
    );
    // Senza l'attesa il primo `search_tool` risponde «connecting» (o nessuno strumento MCP).
    let primo = esiti
        .iter()
        .find(|(n, _)| n == "search_tool")
        .map(|(_, e)| e.to_string());
    assert!(
        primo
            .as_deref()
            .is_some_and(|e| e.contains(r#"\"status\": \"ready\""#)),
        "{primo:?}"
    );
    assert!(esiti.iter().any(|(n, _)| n == "use_tool"), "{esiti:?}");
    assert!(testo.contains("lavoro") && testo.contains("pec"), "{testo}");
    xbot_provider::caldi::chiudi_tutti();
}

/// 2.L-bis: Cursor con il nome vero del catalogo («Posta (IMAP)»): prima `getMcpTools` rispondeva
/// «namespace "Posta (IMAP)" not found»; ora il nome nel `.cursor/mcp.json` è `posta_imap`.
#[tokio::test]
#[ignore]
async fn cursor_con_il_nome_del_catalogo() {
    let c = CursorAgent::nuovo(cursor_agent::trova_nel_path().unwrap())
        .con_cartella_sessioni(cartella("cursor-catalogo"));
    let lento = format!("{}/prove/mcp-lento.py", env!("CARGO_MANIFEST_DIR"));
    let mut r = richiesta(None);
    r.testo = "Usa lo strumento `caselle` del server MCP `posta_imap` ed elenca solo i nomi delle \
               caselle che restituisce, separati da virgole. Non fare altro."
        .into();
    r.mcp = Some(ConfigMcp {
        server: vec![ServerMcp::Stdio {
            nome: "Posta (IMAP)".into(),
            comando: "python3".into(),
            argomenti: vec![lento],
            ambiente: vec![("LENTO_S".into(), SegretoTesto::nuovo("0"))],
        }],
        ..Default::default()
    });
    let dove = c.cartella_di(&r);
    let (strumenti, testo) = turno(&c, r).await;
    println!("cursor: strumenti {strumenti:?}, testo «{testo}»");
    let f = std::fs::read_to_string(dove.join(".cursor/mcp.json")).unwrap();
    assert!(f.contains("\"posta_imap\""), "{f}");
    assert!(strumenti.iter().any(|s| s == "mcp"), "{strumenti:?}");
    assert!(testo.contains("lavoro") && testo.contains("pec"), "{testo}");
}

/// 2.L-ter: come il lavoratore, con la cartella delle sessioni dentro un repository git
/// (la cartella del lavoratore è il worktree del `.git` di XBot). Prima `getMcpTools` dava
/// «matches: []»: `agent` leggeva `.cursor/mcp.json` dalla radice git.
#[tokio::test]
#[ignore]
async fn cursor_con_le_sessioni_dentro_un_repository() {
    let repo = cartella("cursor-repo");
    std::fs::create_dir_all(&repo).unwrap();
    let ok = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .unwrap();
    assert!(ok.success());
    let c = CursorAgent::nuovo(cursor_agent::trova_nel_path().unwrap())
        .con_cartella_sessioni(repo.join("sessioni"));
    let r = richiesta(Some("grok-4.7-low"));
    let dove = c.cartella_di(&r);
    let (strumenti, testo) = turno(&c, r).await;
    println!("cursor: strumenti {strumenti:?}, testo «{testo}»");
    assert!(dove.join(".git").is_dir() && dove.join(".cursor/mcp.json").is_file());
    assert!(strumenti.iter().any(|s| s == "mcp"), "{strumenti:?}");
    assert!(testo.contains("token=xyz"), "{testo}");
}
