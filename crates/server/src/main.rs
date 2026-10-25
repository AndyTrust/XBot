//! `xbot-server`: avvia il motore, oppure `impianta` l'utente.

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use qrcode::QrCode;
use qrcode::render::unicode::Dense1x2;
use tracing_subscriber::EnvFilter;
use xbot_core::{Errore, Risposta, SegretoTesto};
use xbot_db::Db;
use xbot_provider::{ClaudeCode, Finto, Provider, claude_code};
use xbot_server::config::Config;
use xbot_server::registro::{self, Registro};
use xbot_server::{Stato, accesso, app, stato};

#[derive(Parser)]
#[command(name = "xbot-server", version, about = "Il motore di XBot")]
struct Riga {
    #[command(subcommand)]
    comando: Option<Comando>,
}

#[derive(Subcommand)]
enum Comando {
    // --- S2 ---
    /// Ponte stdio↔socket del server MCP interno (lo lancia Claude Code, non a mano).
    McpInterno {
        #[arg(long)]
        socket: std::path::PathBuf,
        #[arg(long)]
        sessione: String,
        /// Facoltativo dalla 2.G (il turno corrente si trova dalla sessione).
        #[arg(long, default_value = "")]
        turno: String,
    },
    // --- fine S2 ---
    // --- S1 ---
    /// Genera le chiavi VAPID delle notifiche push e le stampa per il file d'ambiente.
    Vapid,
    // --- fine S1 ---
    /// Ruota XBOT_SEGRETO_COOKIE: ricifra il segreto TOTP con il valore nuovo (dall'ambiente)
    /// partendo da quello vecchio in XBOT_SEGRETO_COOKIE_VECCHIO. Poi riavvia il server.
    RuotaSegreto,
    /// Crea (o rifà) l'utente: password e, con --totp, il secondo fattore.
    Impianta {
        #[arg(long)]
        nome: String,
        /// Meglio passarla con la variabile XBOT_PASSWORD (non resta nella cronologia).
        #[arg(long, env = "XBOT_PASSWORD", hide_env_values = true)]
        password: String,
        /// Genera il segreto TOTP e stampa l'URL otpauth:// e il QR.
        #[arg(long)]
        totp: bool,
        /// Con --totp: scrive l'URL otpauth:// in questo file (permessi 0600) invece di
        /// stamparlo, così non passa nei log (es. per leggerlo via ssh).
        #[arg(long, requires = "totp", value_name = "PERCORSO")]
        totp_file: Option<std::path::PathBuf>,
        /// Il programma `claude` (se manca si cerca nel PATH).
        #[arg(long)]
        claude: Option<std::path::PathBuf>,
        /// Registra anche il provider `finto` (per provare l'interfaccia senza abbonamento).
        #[arg(long)]
        finto: bool,
    },
}

fn main() -> ExitCode {
    let mut filtro = EnvFilter::try_from_env("XBOT_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    // --- S2 --- a livello debug `rmcp` scriverebbe gli argomenti interi delle chiamate dei
    // canali: resta a `warn` se XBOT_LOG non lo nomina.
    if !std::env::var("XBOT_LOG")
        .unwrap_or_default()
        .contains("rmcp")
        && let Ok(d) = "rmcp=warn".parse()
    {
        filtro = filtro.add_directive(d);
    }
    // --- fine S2 ---
    tracing_subscriber::fmt()
        .with_env_filter(filtro)
        .with_writer(std::io::stderr)
        .init();
    let riga = Riga::parse();
    let esito = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt.block_on(esegui(riga)),
        Err(e) => Err(Errore::interno(format!("runtime: {e}"))),
    };
    match esito {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("errore: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn esegui(riga: Riga) -> Risposta<()> {
    // --- S2 ---
    if let Some(Comando::McpInterno {
        socket,
        sessione,
        turno,
    }) = &riga.comando
    {
        return xbot_server::mcp_interno::ponte::esegui(socket, sessione, turno).await;
    }
    // --- fine S2 ---
    // --- S1 ---
    if let Some(Comando::Vapid) = &riga.comando {
        let (privata, pubblica) = xbot_server::push::genera_chiavi();
        println!("XBOT_VAPID_PRIVATA={privata}\nXBOT_VAPID_PUBBLICA={pubblica}");
        return Ok(());
    }
    // --- fine S1 ---
    let config = Config::da_ambiente()?;
    match riga.comando {
        Some(Comando::Impianta {
            nome,
            password,
            totp,
            totp_file,
            claude,
            finto,
        }) => {
            impianta(
                &config,
                &nome,
                SegretoTesto::from(password),
                totp,
                totp_file.as_deref(),
            )?;
            impianta_provider(&config, claude, finto).await
        }
        Some(Comando::RuotaSegreto) => ruota_segreto(&config),
        // --- S2 --- già servito sopra, prima di leggere la configurazione.
        Some(Comando::McpInterno { .. }) => Ok(()),
        // --- fine S2 ---
        // --- S1 ---
        Some(Comando::Vapid) => Ok(()),
        // --- fine S1 ---
        None => servi(config).await,
    }
}

/// Ricifra il TOTP con il segreto nuovo; il vecchio arriva da `XBOT_SEGRETO_COOKIE_VECCHIO`.
fn ruota_segreto(config: &Config) -> Risposta<()> {
    let vecchio = std::env::var("XBOT_SEGRETO_COOKIE_VECCHIO")
        .map(SegretoTesto::from)
        .map_err(|_| Errore::non_valida("serve XBOT_SEGRETO_COOKIE_VECCHIO nell'ambiente"))?;
    let nuovo = config
        .segreto_cookie
        .as_ref()
        .ok_or_else(|| Errore::non_valida("serve XBOT_SEGRETO_COOKIE nell'ambiente"))?;
    if vecchio.esponi() == nuovo.esponi() {
        return Err(Errore::non_valida("il segreto nuovo è uguale al vecchio"));
    }
    let db = Db::apri(&config.db)?;
    let chiave_vecchia = stato::chiave_totp(&stato::segreto_server(&db, Some(&vecchio))?)?;
    let chiave_nuova = stato::chiave_totp(&stato::segreto_server(&db, Some(nuovo))?)?;
    if accesso::ricifra_totp(&db, &chiave_vecchia, &chiave_nuova)? {
        println!("Segreto TOTP ricifrato con il segreto nuovo. Riavvia il server.");
    } else {
        println!("L'utente non ha il TOTP: niente da ricifrare. Riavvia il server.");
    }
    Ok(())
}

fn impianta(
    config: &Config,
    nome: &str,
    password: SegretoTesto,
    totp: bool,
    totp_file: Option<&std::path::Path>,
) -> Risposta<()> {
    let db = Db::apri(&config.db)?;
    let segreto = stato::segreto_server(&db, config.segreto_cookie.as_ref())?;
    let chiave = stato::chiave_totp(&segreto)?;
    let url = accesso::impianta(&db, &chiave, nome, &password, totp)?;
    println!("Utente «{}» impiantato in {}.", nome.trim(), config.db);
    if let (Some(url), Some(file)) = (&url, totp_file) {
        scrivi_privato(file, url)?;
        println!(
            "Secondo fattore (TOTP): URL otpauth:// scritto in {} (0600). Cancellalo dopo l'uso.",
            file.display()
        );
        return Ok(());
    }
    if let Some(url) = url {
        println!("\nSecondo fattore (TOTP): aggiungilo all'app di autenticazione.\n{url}\n");
        let qr = QrCode::new(url.as_bytes())
            .map_err(|e| Errore::interno(format!("QR: {e}")))?
            .render::<Dense1x2>()
            .dark_color(Dense1x2::Light)
            .light_color(Dense1x2::Dark)
            .build();
        println!("{qr}");
    }
    Ok(())
}

/// Scrive il testo in un file nuovo o troncato, con permessi 0600 fin dalla creazione.
fn scrivi_privato(file: &std::path::Path, testo: &str) -> Risposta<()> {
    use std::io::Write;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let errore = |e: std::io::Error| Errore::interno(format!("{}: {e}", file.display()));
    let mut f = o.open(file).map_err(errore)?;
    #[cfg(unix)]
    {
        // Se il file c'era già, `mode` non si applica: i permessi si stringono comunque.
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(errore)?;
    }
    writeln!(f, "{testo}").map_err(errore)
}

/// Scrive nella tabella `provider` gli adattatori di questa macchina, con i loro modelli.
async fn impianta_provider(
    config: &Config,
    claude: Option<std::path::PathBuf>,
    finto: bool,
) -> Risposta<()> {
    let db = Db::apri(&config.db)?;
    let mut provider: Vec<Arc<dyn Provider>> = Vec::new();
    match claude.or_else(claude_code::trova_nel_path) {
        Some(p) if p.is_file() => {
            println!("Provider claude_code: {}", p.display());
            provider.push(Arc::new(ClaudeCode::nuovo(p)));
        }
        Some(p) => {
            return Err(Errore::non_valida(format!(
                "claude non trovato in {}",
                p.display()
            )));
        }
        None => {
            println!("claude non è nel PATH: provider claude_code non registrato (usa --claude).")
        }
    }
    if finto {
        println!("Provider finto registrato.");
        provider.push(Arc::new(Finto::nuovo()));
    }
    Registro::nuovo(provider).sincronizza(&db).await
}

async fn servi(config: Config) -> Risposta<()> {
    let db = Db::apri(&config.db)?;
    // --- S1 --- servizi della 2.D dall'ambiente (cassaforte, profili, blob, VAPID) e compiti.
    let opzioni = xbot_server::servizi::OpzioniServizi::da_ambiente(&config.db);
    // Senza `XBOT_SESSIONI` le cartelle delle sessioni stanno nella cartella dei dati.
    let sessioni = config
        .sessioni
        .clone()
        .unwrap_or_else(|| opzioni.cartella_dati.join("sessioni"));
    let provider = registro::da_tabella(&db, Some(sessioni)).await?;
    let cookie_sicuro = config.cookie_sicuro();
    let stato = Stato::prepara_servizi(
        db,
        provider,
        config.segreto_cookie.clone(),
        cookie_sicuro,
        config.proxy_fidati.clone(),
        opzioni,
    )
    .await?;
    xbot_server::servizi::avvia_compiti(&stato);
    // --- fine S1 ---
    stato
        .db
        .esegui(|c| {
            xbot_db::eventi::scrivi(
                c,
                "avvio",
                &serde_json::json!({ "versione": xbot_core::VERSIONE }),
            )
        })
        .await?;
    let router = app(stato.clone(), &config.web);
    let ascolto = tokio::net::TcpListener::bind(config.ascolto)
        .await
        .map_err(|e| Errore::interno(format!("ascolto su {}: {e}", config.ascolto)))?;
    let proxy: Vec<String> = config
        .proxy_fidati
        .iter()
        .map(ToString::to_string)
        .collect();
    tracing::info!(
        indirizzo = %config.ascolto,
        db = %config.db,
        cookie_sicuro,
        proxy_fidati = %if proxy.is_empty() { "nessuno".into() } else { proxy.join(",") },
        "XBot in ascolto"
    );
    axum::serve(
        ascolto,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(segnale_di_chiusura(stato))
    .await
    .map_err(|e| Errore::interno(format!("server: {e}")))?;
    tracing::info!("XBot chiusa");
    Ok(())
}

/// Ctrl-C o SIGTERM: i flussi SSE ricevono `chiusura` e finiscono subito; se qualcosa resta
/// aperto, dopo 3 secondi si esce comunque.
async fn segnale_di_chiusura(stato: Stato) {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::error!(errore = %e, "ctrl-c non ascoltabile");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let termina = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(e) => {
                tracing::error!(errore = %e, "SIGTERM non ascoltabile");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let termina = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {}
        () = termina => {}
    }
    tracing::info!("chiusura richiesta");
    stato.chiudi_flussi();
    // --- S2 --- il socket del server MCP interno non resta sul disco.
    stato.motore.s2().togli_socket();
    // --- fine S2 ---
    // --- 2.G --- i processi caldi dei client non sopravvivono al server.
    tokio::task::spawn_blocking(xbot_provider::caldi::chiudi_tutti)
        .await
        .ok();
    // --- 2.H --- i desktop degli assistenti si spengono con il server.
    stato.motore.computer().spegni_tutti().await;
    // --- S1 --- la macchina del server risulta ferma finché non riparte.
    let id = stato.servizi.macchina.id.clone();
    if let Err(e) = stato
        .db
        .esegui(move |c| xbot_db::macchine::ferma(c, &id))
        .await
    {
        tracing::warn!(errore = %e, "macchina non segnata ferma");
    }
    // --- fine S1 ---
    tokio::spawn(async {
        tokio::time::sleep(Duration::from_secs(3)).await;
        tracing::warn!("chiusura forzata: flussi ancora aperti");
        std::process::exit(0);
    });
}
