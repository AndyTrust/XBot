//! `xbot-worker`: enroll, run, install-launchagent, mcp-ponte, --versione.

use std::path::PathBuf;
use std::process::ExitCode;

use tracing_subscriber::EnvFilter;
use xbot_worker::credenziale::{Credenziale, cartella_predefinita, file_credenziale};
use xbot_worker::{Config, esecuzione, iscrizione, launchagent, permessi, scoperta};

const AIUTO: &str = "xbot-worker (XBot, lavoratore dei computer collegati)

  xbot-worker enroll <url> [<biglietto> | -] [--nome mac] [--posti 2]
      (il biglietto anche da XBOT_BIGLIETTO o, con «-», dalla prima riga di stdin:
       così non compare in ps)
  xbot-worker run [--posti 2]
  xbot-worker install-launchagent [--programma <percorso>] [--carica]
  xbot-worker client            (i client trovati nel PATH, con i modelli)
  xbot-worker --versione

Cartella: ~/.xbot/worker (o XBOT_WORKER_CARTELLA, o --cartella); sessioni accanto,
in ../sessioni (o XBOT_SESSIONI). Etichetta del LaunchAgent: XBOT_ETICHETTA_WORKER
(predefinita it.xbot.worker).";

struct Argomenti {
    posizionali: Vec<String>,
    opzioni: Vec<(String, Option<String>)>,
}

impl Argomenti {
    fn da(args: Vec<String>) -> Self {
        let con_valore = [
            "--nome",
            "--posti",
            "--cartella",
            "--programma",
            "--socket",
            "--lavoro",
            "--sessione",
            "--turno",
        ];
        let mut posizionali = Vec::new();
        let mut opzioni = Vec::new();
        let mut i = args.into_iter();
        while let Some(a) = i.next() {
            if con_valore.contains(&a.as_str()) {
                opzioni.push((a, i.next()));
            } else if a.starts_with("--") {
                opzioni.push((a, None));
            } else {
                posizionali.push(a);
            }
        }
        Self {
            posizionali,
            opzioni,
        }
    }

    fn valore(&self, k: &str) -> Option<&str> {
        self.opzioni
            .iter()
            .find(|(n, _)| n == k)
            .and_then(|(_, v)| v.as_deref())
    }

    fn c_e(&self, k: &str) -> bool {
        self.opzioni.iter().any(|(n, _)| n == k)
    }
}

/// Invocato come `claude-mcp-utente`: lancia il `claude` vero senza `--strict-mcp-config`,
/// così il client carica i suoi MCP di utente (l'estensione Claude in Chrome).
fn involucro_claude(args: Vec<String>) -> ExitCode {
    use std::os::unix::process::CommandExt;
    let Some(vero) = scoperta::nel_path("claude") else {
        eprintln!("claude non è nel PATH");
        return ExitCode::from(127);
    };
    let filtrati: Vec<String> = args
        .into_iter()
        .filter(|a| a != "--strict-mcp-config")
        .collect();
    let e = std::process::Command::new(&vero).args(filtrati).exec();
    eprintln!("claude non parte ({}): {e}", vero.display());
    ExitCode::from(126)
}

fn main() -> ExitCode {
    let mut tutti = std::env::args();
    let zero = tutti.next().unwrap_or_default();
    let resto: Vec<String> = tutti.collect();
    let nome = std::path::Path::new(&zero)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if nome == esecuzione::NOME_CLAUDE_UTENTE {
        return involucro_claude(resto);
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("XBOT_LOG").unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
    xbot_worker::prepara_tls();
    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    match rt.block_on(principale(Argomenti::da(resto))) {
        Ok(()) => ExitCode::SUCCESS,
        Err(t) => {
            eprintln!("errore: {t}");
            ExitCode::FAILURE
        }
    }
}

/// Il biglietto di `enroll`: l'argomento, «-» per la prima riga di stdin, altrimenti
/// `XBOT_BIGLIETTO` (l'argomento compare in `ps`, stdin e variabile no).
fn biglietto(
    argomento: Option<&str>,
    ambiente: Option<String>,
    stdin: impl FnOnce() -> std::io::Result<String>,
) -> Option<String> {
    let pulito = |t: String| Some(t.trim().to_string()).filter(|t| !t.is_empty());
    match argomento {
        Some("-") => stdin().ok().and_then(pulito),
        Some(b) => pulito(b.to_string()),
        None => ambiente.and_then(pulito),
    }
}

/// Cosa fare dopo `enroll`, secondo il sistema.
fn suggerimento(os: &str) -> &'static str {
    match os {
        "macos" => {
            "Ora: xbot-worker install-launchagent --carica (oppure installa.sh --modo worker, o xbot-worker run)."
        }
        _ => {
            "Ora avvia il lavoratore come servizio: installa.sh --modo worker (oppure xbot-worker run)."
        }
    }
}

async fn principale(a: Argomenti) -> Result<(), String> {
    let cartella = a
        .valore("--cartella")
        .map(PathBuf::from)
        .unwrap_or_else(cartella_predefinita);
    let posti: u32 = match a.valore("--posti") {
        Some(p) => p.parse().map_err(|_| format!("--posti non valido: {p}"))?,
        None => 2,
    };
    if a.c_e("--versione") || a.c_e("--version") {
        println!("xbot-worker {}", xbot_worker::VERSIONE);
        return Ok(());
    }
    let comando = a.posizionali.first().map(String::as_str).unwrap_or("");
    match comando {
        "enroll" => {
            let uso =
                "uso: xbot-worker enroll <url> [<biglietto> | -] [--nome mac] (o XBOT_BIGLIETTO)";
            let Some(url) = a.posizionali.get(1) else {
                return Err(uso.into());
            };
            let biglietto = biglietto(
                a.posizionali.get(2).map(String::as_str),
                std::env::var("XBOT_BIGLIETTO").ok(),
                || {
                    let mut r = String::new();
                    std::io::stdin().read_line(&mut r).map(|_| r)
                },
            )
            .ok_or(uso)?;
            let biglietto = &biglietto;
            let provider = scoperta::scopri().await;
            let nome = a.valore("--nome").unwrap_or("mac");
            let c = iscrizione::iscrivi(
                &cartella,
                iscrizione::Iscrizione {
                    url,
                    biglietto,
                    nome,
                    posti,
                    provider,
                },
            )
            .await
            .map_err(|e| e.to_string())?;
            println!(
                "Computer collegato: macchina {} su {}.\nCredenziale in {} (0600).\n{}",
                c.macchina_id,
                c.url,
                file_credenziale(&cartella).display(),
                suggerimento(std::env::consts::OS)
            );
            Ok(())
        }
        "run" => {
            let cred =
                Credenziale::leggi(&file_credenziale(&cartella)).map_err(|e| e.to_string())?;
            let mut conf = Config::predefinita();
            conf.sessioni = xbot_worker::credenziale::cartella_sessioni(&cartella);
            conf.cartella = cartella;
            conf.posti = posti;
            conf.consenti_finto = a.c_e("--finto");
            let (tx, rx) = tokio::sync::watch::channel(false);
            tokio::spawn(async move {
                // Ctrl-C o SIGTERM (launchd): il canale chiude e i processi caldi si fermano.
                match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                    Ok(mut t) => {
                        tokio::select! {
                            _ = tokio::signal::ctrl_c() => {}
                            _ = t.recv() => {}
                        }
                    }
                    Err(_) => {
                        let _ = tokio::signal::ctrl_c().await;
                    }
                }
                let _ = tx.send(true);
            });
            let r = xbot_worker::canale::esegui(conf, cred, rx)
                .await
                .map_err(|e| e.to_string());
            tokio::task::spawn_blocking(xbot_provider::caldi::chiudi_tutti)
                .await
                .ok();
            r
        }
        "install-launchagent" => {
            let programma = match a.valore("--programma") {
                Some(p) => PathBuf::from(p),
                None => std::env::current_exe().map_err(|e| e.to_string())?,
            };
            let file = launchagent::installa(&programma, &cartella).map_err(|e| e.to_string())?;
            println!("LaunchAgent scritto: {}", file.display());
            let uid = std::process::Command::new("id")
                .arg("-u")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();
            let carica = format!("launchctl bootstrap gui/{uid} {}", file.display());
            if a.c_e("--carica") {
                let _ = std::process::Command::new("launchctl")
                    .args([
                        "bootout",
                        &format!("gui/{uid}/{}", launchagent::etichetta()),
                    ])
                    .status();
                let s = std::process::Command::new("launchctl")
                    .args(["bootstrap", &format!("gui/{uid}"), &file.to_string_lossy()])
                    .status()
                    .map_err(|e| e.to_string())?;
                if !s.success() {
                    return Err(format!("{carica}: {s}"));
                }
                println!("Avviato ({}).", launchagent::etichetta());
            } else {
                println!("Per avviarlo: {carica}");
            }
            Ok(())
        }
        "client" => {
            let p = scoperta::scopri().await;
            println!("{}", serde_json::to_string_pretty(&p).unwrap_or_default());
            Ok(())
        }
        "mcp-ponte" => {
            let sessione = a.valore("--sessione");
            let lavoro = a.valore("--lavoro").or(sessione.map(|_| ""));
            let (Some(s), Some(l)) = (a.valore("--socket"), lavoro) else {
                return Err(
                    "uso: xbot-worker mcp-ponte --socket <s> (--sessione <id> | --lavoro <id>)"
                        .into(),
                );
            };
            permessi::ponte(std::path::Path::new(s), l, a.valore("--turno"), sessione)
                .await
                .map_err(|e| e.to_string())
        }
        "" | "aiuto" | "help" | "--help" => {
            println!("{AIUTO}");
            Ok(())
        }
        altro => Err(format!("comando sconosciuto: {altro}\n\n{AIUTO}")),
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn biglietto_da_argomento_stdin_o_ambiente() {
        let mai = || -> std::io::Result<String> { panic!("stdin non serve") };
        assert_eq!(
            biglietto(Some("abc"), Some("env".into()), mai).as_deref(),
            Some("abc")
        );
        assert_eq!(
            biglietto(Some("-"), None, || Ok("  dal-tubo \n".into())).as_deref(),
            Some("dal-tubo")
        );
        assert_eq!(
            biglietto(None, Some(" env ".into()), mai).as_deref(),
            Some("env")
        );
        assert_eq!(biglietto(None, None, mai), None);
        assert_eq!(biglietto(Some("-"), None, || Ok("\n".into())), None);
    }

    #[test]
    fn suggerimento_per_sistema() {
        assert!(suggerimento("macos").contains("install-launchagent"));
        assert!(suggerimento("linux").contains("installa.sh --modo worker"));
        assert!(!suggerimento("linux").contains("launchagent"));
    }
}
