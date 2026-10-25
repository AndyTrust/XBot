//! Le parti comuni degli adattatori dei client «esterni» (Fase 2.E): `cursor_agent`, `codex`,
//! `gemini`. Il processo si lancia con l'ambiente ripulito, stdout si legge riga per riga e
//! un `Traduttore` (senza processo, si prova con i campioni) trasforma le righe in `EventoTurno`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};
use xbot_core::{Codice, Errore, SegretoTesto};

use crate::claude_code::mcp::{variabile_valida, variabile_vietata};
use crate::{EventoTurno, ManigliaTurno, RichiestaTurno};

/// Quanto stderr si tiene per il messaggio d'errore.
const STDERR_MAX: usize = 8 * 1024;

/// Variabili della sessione madre e chiavi a consumo: il figlio usa l'abbonamento del client.
const AMBIENTE_DA_TOGLIERE: [&str; 20] = [
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_HOST_SESSION_ID",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_AGENT_SDK_VERSION",
    "CLAUDE_EFFORT",
    "CLAUDE_PID",
    "ANTHROPIC_API_KEY",
    "OPENAI_API_KEY",
    "CODEX_API_KEY",
    "CURSOR_API_KEY",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "GOOGLE_GENAI_USE_VERTEXAI",
    "XAI_API_KEY",
];

/// Il permesso di esecuzione della macchina, letto da `RichiestaTurno.permessi`.
///
/// Valori: `never` | `ask` | `always` (come `macchina.permesso_esecuzione`); si accettano anche
/// i modi di Claude Code `plan` (= mai) e `bypassPermissions` (= sempre). Il resto vale `ask`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permesso {
    Mai,
    Chiedi,
    Sempre,
}

impl Permesso {
    pub fn di(r: &RichiestaTurno) -> Self {
        match r.permessi.as_deref() {
            Some("never" | "plan") => Self::Mai,
            Some("always" | "bypassPermissions") => Self::Sempre,
            _ => Self::Chiedi,
        }
    }
}

/// Il testo del messaggio: sistema in cornice `<sistema>` (i client non hanno un'opzione),
/// poi il testo dell'utente, poi gli allegati come percorsi. Mai un NUL (il processo non partirebbe).
pub fn testo_del_turno(r: &RichiestaTurno) -> String {
    let mut t = String::new();
    if let Some(s) = r.sistema.as_deref().filter(|s| !s.trim().is_empty()) {
        t.push_str("<sistema>\n");
        t.push_str(s.trim());
        t.push_str("\n</sistema>\n\n");
    }
    t.push_str(&r.testo);
    for p in &r.allegati {
        t.push_str(&format!("\n\n[allegato: {}]", p.display()));
    }
    t.replace('\0', "")
}

/// Cerca il primo dei programmi nel `PATH` (senza shell).
pub fn trova_nel_path(nomi: &[&str]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let cartelle: Vec<PathBuf> = std::env::split_paths(&path).collect();
    nomi.iter()
        .flat_map(|n| cartelle.iter().map(move |d| d.join(n)))
        .find(|p| p.is_file())
}

/// Toglie i caratteri invisibili e gli spazi doppi (le etichette di `agent --list-models`).
pub fn pulisci_etichetta(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '\u{200b}'..='\u{200f}' | '\u{feff}' | '\u{2060}'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Il traduttore delle righe di un client.
pub trait Traduttore: Send + 'static {
    fn riga(&mut self, linea: &str) -> Vec<EventoTurno>;
    /// Vero dopo la riga che chiude il turno.
    fn finito(&self) -> bool;
}

/// Un processo da lanciare per un turno.
pub struct Lancio {
    /// Il nome del client nei messaggi («agent», «codex», «gemini»).
    pub nome: &'static str,
    pub programma: PathBuf,
    pub argomenti: Vec<OsString>,
    pub cartella: PathBuf,
    pub ambiente: Vec<(String, SegretoTesto)>,
    /// Variabili decise dall'adattatore (non dall'utente), messe per ultime e senza il filtro dei
    /// nomi vietati: la casa isolata di grok usa `HOME` (Fase 2.G-bis).
    pub ambiente_adattatore: Vec<(String, String)>,
    /// Scritto su stdin e poi chiuso; `None` = stdin vuoto.
    pub stdin: Option<String>,
    pub silenzio: Duration,
    pub attesa_kill: Duration,
}

/// Le variabili `XBOT_*` che un turno può passare apposta (il resto delle `XBOT_*` è del server:
/// cassaforte, cookie, VAPID…).
pub const XBOT_DEL_TURNO: [&str; 2] = ["XBOT_COMPUTER_HOME", "XBOT_MCP_GETTONE"];

/// Una variabile ereditata da XBot che il client non deve vedere: tutte le `XBOT_*`, ogni
/// `*_API_KEY` e le variabili di Claude Code e delle chiavi (Fase 2.G-bis, §22).
pub fn ereditata_vietata(nome: &str) -> bool {
    nome.starts_with("XBOT_")
        || nome.ends_with("_API_KEY")
        || nome.starts_with("CLAUDE_CODE_")
        || AMBIENTE_DA_TOGLIERE.contains(&nome)
}

/// Toglie dall'ambiente ereditato le variabili di [`ereditata_vietata`].
pub fn pulisci(cmd: &mut Command) {
    let gia: Vec<OsString> = cmd.as_std().get_envs().map(|(k, _)| k.to_owned()).collect();
    for k in std::env::vars_os().map(|(k, _)| k).chain(gia) {
        if k.to_str().is_none_or(ereditata_vietata) {
            cmd.env_remove(&k);
        }
    }
    for k in AMBIENTE_DA_TOGLIERE {
        cmd.env_remove(k);
    }
}

/// Un nome che la richiesta può mettere nell'ambiente del client (i segreti concessi, `DISPLAY`,
/// `XBOT_COMPUTER_HOME`).
pub fn ammessa(nome: &str) -> bool {
    variabile_valida(nome) && (!variabile_vietata(nome) || XBOT_DEL_TURNO.contains(&nome))
}

/// [`pulisci`] e poi l'ambiente della richiesta (solo i nomi di [`ammessa`]).
pub fn metti_ambiente(cmd: &mut Command, ambiente: &[(String, SegretoTesto)]) {
    pulisci(cmd);
    for (k, v) in ambiente {
        if ammessa(k) {
            cmd.env(k, v.esponi());
        } else {
            tracing::warn!(variabile = %k, "nome di variabile non valido: saltata");
        }
    }
}

fn comando(l: &Lancio) -> Command {
    let mut cmd = Command::new(&l.programma);
    cmd.args(&l.argomenti)
        .current_dir(&l.cartella)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    cmd.env("TERM", "dumb").env("NO_COLOR", "1").env("CI", "1");
    metti_ambiente(&mut cmd, &l.ambiente);
    for (k, v) in &l.ambiente_adattatore {
        cmd.env(k, v);
    }
    cmd
}

/// Lancia il processo con stdin, stdout e stderr in tubo (per i processi caldi, Fase 2.G).
/// stderr si scarta in un compito a parte, così il processo non si blocca a tubo pieno.
pub async fn lancia(lancio: &Lancio) -> Result<Child, Errore> {
    tokio::fs::create_dir_all(&lancio.cartella)
        .await
        .map_err(|e| {
            Errore::interno(format!(
                "cartella di lavoro {}: {e}",
                lancio.cartella.display()
            ))
        })?;
    let mut figlio = comando(lancio).spawn().map_err(|e| {
        Errore::nuovo(
            Codice::ProviderFermo,
            format!(
                "{} non parte ({}): {e}",
                lancio.nome,
                lancio.programma.display()
            ),
        )
    })?;
    if let Some(mut e) = figlio.stderr.take() {
        tokio::spawn(async move {
            let _ = tokio::io::copy(&mut e, &mut tokio::io::sink()).await;
        });
    }
    tracing::info!(pid = ?figlio.id(), client = lancio.nome, "client caldo avviato");
    Ok(figlio)
}

/// Lancia il processo e legge il turno in un compito a parte.
pub async fn avvia<T: Traduttore>(
    lancio: Lancio,
    traduttore: T,
    eventi: mpsc::Sender<EventoTurno>,
) -> Result<ManigliaTurno, Errore> {
    tokio::fs::create_dir_all(&lancio.cartella)
        .await
        .map_err(|e| {
            Errore::interno(format!(
                "cartella di lavoro {}: {e}",
                lancio.cartella.display()
            ))
        })?;
    let figlio = comando(&lancio).spawn().map_err(|e| {
        Errore::nuovo(
            Codice::ProviderFermo,
            format!(
                "{} non parte ({}): {e}",
                lancio.nome,
                lancio.programma.display()
            ),
        )
    })?;
    let pid = figlio.id();
    tracing::info!(pid = ?pid, client = lancio.nome, "client avviato");
    let (ferma_tx, ferma_rx) = oneshot::channel();
    let turno = Turno {
        nome: lancio.nome,
        figlio,
        stdin: lancio.stdin,
        eventi,
        stop: ferma_rx,
        silenzio: lancio.silenzio,
        attesa_kill: lancio.attesa_kill,
        traduttore,
    };
    tokio::spawn(turno.esegui());
    Ok(ManigliaTurno {
        pid,
        interrompi: Box::new(move || {
            let _ = ferma_tx.send(());
        }),
    })
}

fn segnale(pid: Option<u32>, s: Signal) {
    if let Some(p) = pid.and_then(|p| i32::try_from(p).ok())
        && let Err(e) = kill(Pid::from_raw(p), s)
    {
        tracing::debug!(pid = p, errore = %e, "segnale non consegnato");
    }
}

/// SIGTERM, poi SIGKILL se dopo `attesa` il processo è ancora vivo.
async fn ferma(figlio: &mut Child, attesa: Duration) {
    segnale(figlio.id(), Signal::SIGTERM);
    if tokio::time::timeout(attesa, figlio.wait()).await.is_err() {
        tracing::warn!(pid = ?figlio.id(), "il client non si ferma: SIGKILL");
        if let Err(e) = figlio.kill().await {
            tracing::warn!(errore = %e, "SIGKILL fallito");
        }
    }
}

enum Esito {
    Fine,
    Interrotto,
    Silenzio,
    Errore(String),
}

struct Turno<T> {
    nome: &'static str,
    figlio: Child,
    stdin: Option<String>,
    eventi: mpsc::Sender<EventoTurno>,
    stop: oneshot::Receiver<()>,
    silenzio: Duration,
    attesa_kill: Duration,
    traduttore: T,
}

impl<T: Traduttore> Turno<T> {
    async fn esegui(mut self) {
        if let Some(mut stdin) = self.figlio.stdin.take() {
            if let Some(t) = self.stdin.take()
                && let Err(e) = stdin.write_all(t.as_bytes()).await
            {
                tracing::warn!(errore = %e, "stdin del client non scritto");
            }
            drop(stdin);
        }
        let stderr = self.figlio.stderr.take().map(|mut e| {
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let _ = (&mut e)
                    .take(STDERR_MAX as u64 * 16)
                    .read_to_end(&mut buf)
                    .await;
                coda_stderr(&String::from_utf8_lossy(&buf))
            })
        });
        let esito = match self.figlio.stdout.take() {
            Some(out) => self.leggi(out).await,
            None => Esito::Errore("stdout non disponibile".into()),
        };
        match esito {
            Esito::Fine if self.traduttore.finito() => {
                drop(self.eventi);
                if tokio::time::timeout(self.attesa_kill, self.figlio.wait())
                    .await
                    .is_err()
                {
                    ferma(&mut self.figlio, self.attesa_kill).await;
                }
            }
            Esito::Interrotto => {
                drop(self.eventi);
                ferma(&mut self.figlio, self.attesa_kill).await;
            }
            Esito::Silenzio => {
                ferma(&mut self.figlio, self.attesa_kill).await;
                let minuti = self.silenzio.as_secs_f64() / 60.0;
                let _ = self
                    .eventi
                    .send(EventoTurno::Fallito {
                        codice: "silenzio".into(),
                        testo: format!(
                            "{} non scrive da {minuti:.1} minuti: interrotto",
                            self.nome
                        ),
                    })
                    .await;
            }
            Esito::Fine | Esito::Errore(_) => {
                let motivo = match esito {
                    Esito::Errore(t) => t,
                    _ => String::new(),
                };
                let stato = match tokio::time::timeout(self.attesa_kill, self.figlio.wait()).await {
                    Ok(Ok(s)) => s.to_string(),
                    Ok(Err(e)) => format!("attesa fallita: {e}"),
                    Err(_) => {
                        ferma(&mut self.figlio, self.attesa_kill).await;
                        "non uscito, fermato".into()
                    }
                };
                let err = match stderr {
                    Some(h) => h.await.unwrap_or_default(),
                    None => String::new(),
                };
                let mut testo = format!("{} è uscito senza risultato ({stato})", self.nome);
                for parte in [motivo, err] {
                    if !parte.is_empty() {
                        testo.push_str(": ");
                        testo.push_str(&parte);
                    }
                }
                let _ = self
                    .eventi
                    .send(EventoTurno::Fallito {
                        codice: "processo_uscito".into(),
                        testo,
                    })
                    .await;
            }
        }
    }

    async fn leggi(&mut self, out: tokio::process::ChildStdout) -> Esito {
        let mut righe = BufReader::new(out).lines();
        let mut ascolta_stop = true;
        loop {
            let prossima = tokio::select! {
                r = &mut self.stop, if ascolta_stop => {
                    if r.is_ok() {
                        return Esito::Interrotto;
                    }
                    ascolta_stop = false;
                    continue;
                }
                r = tokio::time::timeout(self.silenzio, righe.next_line()) => r,
            };
            match prossima {
                Err(_) => return Esito::Silenzio,
                Ok(Err(e)) => return Esito::Errore(format!("lettura di stdout: {e}")),
                Ok(Ok(None)) => return Esito::Fine,
                Ok(Ok(Some(linea))) => {
                    for ev in self.traduttore.riga(&linea) {
                        if self.eventi.send(ev).await.is_err() {
                            return Esito::Interrotto;
                        }
                    }
                    if self.traduttore.finito() {
                        return Esito::Fine;
                    }
                }
            }
        }
    }
}

/// Le righe utili di stderr: senza le pile di chiamate (`at …`), al massimo `STDERR_MAX` byte
/// dalla fine.
pub fn coda_stderr(t: &str) -> String {
    let utili: Vec<&str> = t
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim_start().starts_with("at ") && !l.trim().is_empty())
        .collect();
    let t = utili.join("\n");
    let inizio = t.len().saturating_sub(STDERR_MAX);
    let inizio = (inizio..t.len())
        .find(|i| t.is_char_boundary(*i))
        .unwrap_or(t.len());
    t[inizio..].trim().to_string()
}

/// La cartella di lavoro del turno: quella della sessione o una vuota sotto `sessioni/<id>`.
pub fn cartella_di(sessioni: &Path, r: &RichiestaTurno) -> PathBuf {
    r.cartella_lavoro
        .clone()
        .unwrap_or_else(|| sessioni.join(&r.sessione_id))
}

/// Un intero da un valore JSON (numero o testo).
pub fn intero(v: &serde_json::Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

#[cfg(test)]
mod test {
    use super::*;

    /// §22: il processo vede solo le variabili ammesse.
    #[tokio::test]
    async fn ambiente_pulito_nel_processo() {
        let mut cmd = Command::new("/usr/bin/env");
        cmd.env("XBOT_CASSAFORTE_CHIAVE", "segreto")
            .env("XBOT_SEGRETO_COOKIE", "segreto")
            .env("OPENAI_API_KEY", "chiave")
            .env("CLAUDE_CODE_SESSION_ID", "x");
        metti_ambiente(
            &mut cmd,
            &[
                ("DISPLAY".into(), SegretoTesto::nuovo(":50")),
                ("XBOT_COMPUTER_HOME".into(), SegretoTesto::nuovo("/c")),
                ("GITHUB_TOKEN".into(), SegretoTesto::nuovo("concesso")),
                ("XBOT_CASSAFORTE_ALTRO".into(), SegretoTesto::nuovo("no")),
                ("HOME".into(), SegretoTesto::nuovo("/no")),
            ],
        );
        let o = cmd.output().await.unwrap();
        let t = String::from_utf8_lossy(&o.stdout);
        for no in [
            "XBOT_CASSAFORTE",
            "XBOT_SEGRETO_COOKIE",
            "OPENAI_API_KEY",
            "CLAUDE_CODE_",
            "HOME=/no",
        ] {
            assert!(!t.contains(no), "{no} in {t}");
        }
        for si in [
            "DISPLAY=:50",
            "XBOT_COMPUTER_HOME=/c",
            "GITHUB_TOKEN=concesso",
        ] {
            assert!(t.contains(si), "{si} manca in {t}");
        }
        assert!(ereditata_vietata("XBOT_VAPID_PRIVATA") && ereditata_vietata("XAI_API_KEY"));
        assert!(!ereditata_vietata("PATH") && !ereditata_vietata("DISPLAY"));
    }

    #[test]
    fn sistema_in_cornice_e_allegati() {
        let r = RichiestaTurno {
            sistema: Some("sei Postino\0".into()),
            testo: "ciao".into(),
            allegati: vec![PathBuf::from("/tmp/a.png")],
            ..Default::default()
        };
        assert_eq!(
            testo_del_turno(&r),
            "<sistema>\nsei Postino\n</sistema>\n\nciao\n\n[allegato: /tmp/a.png]"
        );
        let r = RichiestaTurno {
            testo: "solo".into(),
            ..Default::default()
        };
        assert_eq!(testo_del_turno(&r), "solo");
    }

    #[test]
    fn permessi_dalla_richiesta() {
        let p = |s: Option<&str>| {
            Permesso::di(&RichiestaTurno {
                permessi: s.map(str::to_string),
                ..Default::default()
            })
        };
        assert_eq!(p(None), Permesso::Chiedi);
        assert_eq!(p(Some("never")), Permesso::Mai);
        assert_eq!(p(Some("plan")), Permesso::Mai);
        assert_eq!(p(Some("always")), Permesso::Sempre);
        assert_eq!(p(Some("acceptEdits")), Permesso::Chiedi);
    }

    #[test]
    fn etichette_pulite() {
        assert_eq!(
            pulisci_etichetta("Grok 4.7  Low Fast\u{200b}\u{200b}"),
            "Grok 4.7 Low Fast"
        );
    }

    #[test]
    fn stderr_senza_pile() {
        let t = "Errore: niente\n    at a (x.js:1)\n    at b (y.js:2)\n\nfine";
        assert_eq!(coda_stderr(t), "Errore: niente\nfine");
    }
}

/// Aiuti delle prove degli adattatori esterni.
#[cfg(test)]
pub mod prove_comuni {
    use std::path::PathBuf;
    use std::time::Duration;

    use tokio::sync::mpsc;

    use crate::{EventoTurno, Provider, RichiestaTurno};

    pub fn finto(nome: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("prove")
            .join(nome)
    }

    pub fn cartella_prova(nome: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("xbot-prova-2e-a-{nome}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Avvia un turno e raccoglie gli eventi fino alla chiusura (30 s al massimo).
    pub async fn turno(p: &dyn Provider, r: RichiestaTurno) -> Vec<EventoTurno> {
        let (tx, mut rx) = mpsc::channel(64);
        let _m = p.avvia(r, tx).await.unwrap();
        let mut v = Vec::new();
        while let Ok(Some(e)) = tokio::time::timeout(Duration::from_secs(30), rx.recv()).await {
            v.push(e);
        }
        v
    }

    pub fn testo(ev: &[EventoTurno]) -> String {
        let mut t = String::new();
        for e in ev {
            match e {
                EventoTurno::TestoDelta(d) => t.push_str(d),
                EventoTurno::TestoCompleto(c) => t = c.clone(),
                _ => {}
            }
        }
        t
    }

    pub fn campione(nome: &str) -> String {
        std::fs::read_to_string(finto(nome)).unwrap()
    }
}
