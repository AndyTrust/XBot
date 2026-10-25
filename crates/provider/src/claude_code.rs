//! L'adattatore `claude_code`: lancia il `claude` ufficiale in modalità `-p` con `stream-json`
//! in ingresso e in uscita, scrive il messaggio su stdin e traduce stdout in `EventoTurno`.
//! Dalla Fase 2.G il processo resta vivo fra i turni della sessione (`caldi`): `--resume` serve
//! solo alla prima apertura di una sessione già esistente.

mod flusso;
pub mod mcp;
mod riga_comando;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};
use xbot_core::{Codice, Errore};

pub use flusso::Lettore;
pub use riga_comando::{PERMESSI_PREDEFINITI, argomenti, messaggio_utente, modo_permessi};

use crate::{EventoTurno, ManigliaTurno, Modello, Provider, Quota, RichiestaTurno, caldi};

pub const ID: &str = "claude_code";
const LIVELLI: [&str; 4] = ["low", "medium", "high", "max"];
/// Variabili d'ambiente di una sessione Claude Code «madre»: se il server parte da dentro
/// una sessione, il `claude` figlio non deve credersi annidato.
const AMBIENTE_DA_TOGLIERE: [&str; 12] = [
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
];
/// Quanto stderr si tiene per il messaggio d'errore.
const STDERR_MAX: usize = 8 * 1024;

#[derive(Debug, Clone)]
pub struct ClaudeCode {
    programma: PathBuf,
    sessioni: PathBuf,
    silenzio: Duration,
    attesa_kill: Duration,
}

/// Le cartelle di lavoro vuote delle sessioni: `XBOT_SESSIONI`, altrimenti
/// `<XBOT_DATI>/sessioni`, altrimenti `~/.xbot/sessioni`.
pub fn cartella_sessioni_predefinita() -> PathBuf {
    std::env::var_os("XBOT_SESSIONI")
        .filter(|d| !d.is_empty())
        .map_or_else(
            || xbot_core::cartella_dati_predefinita().join("sessioni"),
            PathBuf::from,
        )
}

/// Cerca `claude` nel `PATH` (come `which`, senza shell: in zsh `claude` può essere una funzione).
pub fn trova_nel_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("claude"))
        .find(|p| p.is_file())
}

impl ClaudeCode {
    pub fn nuovo(programma: impl Into<PathBuf>) -> Self {
        Self {
            programma: programma.into(),
            sessioni: cartella_sessioni_predefinita(),
            silenzio: Duration::from_secs(600),
            attesa_kill: Duration::from_secs(5),
        }
    }

    pub fn con_cartella_sessioni(mut self, cartella: impl Into<PathBuf>) -> Self {
        self.sessioni = cartella.into();
        self
    }

    /// Dopo quanto silenzio su stdout il turno si interrompe (10 minuti di norma).
    pub fn con_silenzio(mut self, silenzio: Duration) -> Self {
        self.silenzio = silenzio;
        self
    }

    /// La cartella di lavoro del turno: quella della sessione o una vuota sotto `sessioni/<id>`.
    pub fn cartella_di(&self, r: &RichiestaTurno) -> PathBuf {
        r.cartella_lavoro
            .clone()
            .unwrap_or_else(|| self.sessioni.join(&r.sessione_id))
    }
}

/// Il processo `claude` caldo di una sessione: più righe utente sullo stesso stdin; il `result`
/// chiude il turno, non il processo.
struct Processo {
    figlio: Child,
    stdin: Option<tokio::process::ChildStdin>,
    righe: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    stderr: Option<tokio::task::JoinHandle<String>>,
    silenzio: Duration,
    attesa_kill: Duration,
    /// `total_cost_usd` è cumulativo per processo: si tiene il totale per dare il costo del turno.
    costo_finora: f64,
}

enum Esito {
    Fine,
    Interrotto,
    Silenzio,
    Errore(String),
}

impl Processo {
    fn nuovo(mut figlio: Child, silenzio: Duration, attesa_kill: Duration) -> Result<Self, Errore> {
        let stdout = figlio
            .stdout
            .take()
            .ok_or_else(|| Errore::interno("stdout di claude non disponibile"))?;
        let stderr = figlio.stderr.take().map(|mut e| {
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let _ = (&mut e)
                    .take(STDERR_MAX as u64 * 4)
                    .read_to_end(&mut buf)
                    .await;
                let t = String::from_utf8_lossy(&buf).into_owned();
                let inizio = t.len().saturating_sub(STDERR_MAX);
                let inizio = (inizio..t.len())
                    .find(|i| t.is_char_boundary(*i))
                    .unwrap_or(t.len());
                t[inizio..].trim().to_string()
            })
        });
        Ok(Self {
            stdin: figlio.stdin.take(),
            figlio,
            righe: BufReader::new(stdout).lines(),
            stderr,
            silenzio,
            attesa_kill,
            costo_finora: 0.0,
        })
    }

    async fn leggi(
        &mut self,
        lettore: &mut Lettore,
        eventi: &mpsc::Sender<EventoTurno>,
        stop: &mut oneshot::Receiver<()>,
    ) -> Esito {
        // Se la maniglia viene lasciata cadere senza interrompere, il turno continua.
        let mut ascolta_stop = true;
        loop {
            let prossima = tokio::select! {
                r = &mut *stop, if ascolta_stop => {
                    if r.is_ok() {
                        return Esito::Interrotto;
                    }
                    ascolta_stop = false;
                    continue;
                }
                r = tokio::time::timeout(self.silenzio, self.righe.next_line()) => r,
            };
            match prossima {
                Err(_) => return Esito::Silenzio,
                Ok(Err(e)) => return Esito::Errore(format!("lettura di stdout: {e}")),
                Ok(Ok(None)) => return Esito::Fine,
                Ok(Ok(Some(linea))) => {
                    for mut ev in lettore.riga(&linea) {
                        if let EventoTurno::Finito { costo_usd, .. } = &mut ev
                            && let Some(c) = costo_usd
                        {
                            let totale = *c;
                            *c = (totale - self.costo_finora).max(0.0);
                            self.costo_finora = totale;
                        }
                        if eventi.send(ev).await.is_err() {
                            return Esito::Interrotto;
                        }
                    }
                    if lettore.finito() {
                        return Esito::Fine;
                    }
                }
            }
        }
    }
}

#[async_trait]
impl caldi::Conversazione for Processo {
    type Messaggio = String;

    async fn turno(&mut self, l: caldi::Lavoro<String>) -> caldi::DopoTurno {
        let caldi::Lavoro {
            messaggio,
            eventi,
            mut stop,
        } = l;
        let scritto = match self.stdin.as_mut() {
            Some(s) => {
                let riga = format!("{messaggio}\n");
                match s.write_all(riga.as_bytes()).await {
                    Ok(()) => s.flush().await.map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                }
            }
            None => Err("stdin chiuso".into()),
        };
        if let Err(e) = &scritto {
            tracing::warn!(errore = %e, "stdin di claude non scritto");
        }
        if !caldi::accesi() {
            // Spento: un turno solo, poi stdin chiuso (come prima della 2.G).
            self.stdin = None;
        }
        let mut lettore = Lettore::nuovo();
        let esito = self.leggi(&mut lettore, &eventi, &mut stop).await;
        match esito {
            Esito::Fine if lettore.finito() => caldi::DopoTurno::Pronto,
            Esito::Interrotto => {
                drop(eventi);
                caldi::ferma(&mut self.figlio, self.attesa_kill).await;
                caldi::DopoTurno::Chiudi
            }
            Esito::Silenzio => {
                caldi::ferma(&mut self.figlio, self.attesa_kill).await;
                let minuti = self.silenzio.as_secs_f64() / 60.0;
                let _ = eventi
                    .send(EventoTurno::Fallito {
                        codice: "silenzio".into(),
                        testo: format!("claude non scrive da {minuti:.1} minuti: interrotto"),
                    })
                    .await;
                caldi::DopoTurno::Chiudi
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
                        caldi::ferma(&mut self.figlio, self.attesa_kill).await;
                        "non uscito, fermato".into()
                    }
                };
                let err = match self.stderr.take() {
                    Some(h) => h.await.unwrap_or_default(),
                    None => String::new(),
                };
                let mut testo = format!("claude è uscito senza risultato ({stato})");
                for parte in [motivo, err] {
                    if !parte.is_empty() {
                        testo.push_str(": ");
                        testo.push_str(&parte);
                    }
                }
                let _ = eventi
                    .send(EventoTurno::Fallito {
                        codice: "processo_uscito".into(),
                        testo,
                    })
                    .await;
                caldi::DopoTurno::Chiudi
            }
        }
    }

    async fn uscito(&mut self) {
        let _ = self.figlio.wait().await;
    }

    async fn chiudi(&mut self) {
        self.stdin = None;
        // Senza stdin `claude -p` esce da solo; se non esce, si ferma.
        if tokio::time::timeout(self.attesa_kill, self.figlio.wait())
            .await
            .is_err()
        {
            caldi::ferma(&mut self.figlio, self.attesa_kill).await;
        }
    }
}

/// La firma del processo: tutto quello che, se cambia, vuole un processo nuovo.
fn firma(programma: &Path, cartella: &Path, r: &RichiestaTurno) -> u64 {
    let mut f = caldi::Firma::default()
        .con(programma)
        .con(cartella)
        .con(&r.modello)
        .con(&r.livello)
        .con(&r.permessi)
        .con(&r.sistema);
    for (k, v) in &r.ambiente {
        f = f.con(k).con(v.esponi());
    }
    if let Some(m) = &r.mcp {
        f = f
            .con(mcp::json_config(m).to_string())
            .con(&m.strumento_permessi);
    }
    f.fine()
}

fn modello(id: &str, etichetta: &str, dettaglio: &str) -> Modello {
    Modello {
        id: id.into(),
        etichetta: etichetta.into(),
        dettaglio: Some(dettaglio.into()),
        livelli: LIVELLI.iter().map(|l| l.to_string()).collect(),
    }
}

#[async_trait]
impl Provider for ClaudeCode {
    fn id(&self) -> &str {
        ID
    }

    fn nome(&self) -> &str {
        "Claude Code"
    }

    fn programma(&self) -> &str {
        self.programma.to_str().unwrap_or("claude")
    }

    async fn modelli(&self) -> Result<Vec<Modello>, Errore> {
        Ok(vec![
            modello("claude-opus-5-5", "Opus 5.5", "il più capace"),
            modello("claude-sonnet-5-5", "Sonnet 5.5", "equilibrato"),
            modello("claude-haiku-5-5", "Haiku 5.5", "il più veloce"),
        ])
    }

    /// La quota arriva con gli eventi `Quota` di ogni turno.
    async fn account(&self) -> Result<Option<crate::Account>, Errore> {
        let (_, t) = crate::accesso::uscita(&self.programma, &["auth", "status", "--json"]).await?;
        Ok(crate::accesso::leggi_claude(&t))
    }

    fn comando_accesso(&self) -> Option<crate::ComandoAccesso> {
        let mut c = crate::ComandoAccesso::nuovo(&self.programma, &["auth", "login", "--claudeai"]);
        c.chiede_codice = true;
        Some(c)
    }

    fn comando_uscita(&self) -> Option<crate::ComandoAccesso> {
        Some(crate::ComandoAccesso::nuovo(
            &self.programma,
            &["auth", "logout"],
        ))
    }

    async fn quota(&self) -> Result<Option<Quota>, Errore> {
        Ok(None)
    }

    async fn avvia(
        &self,
        mut richiesta: RichiestaTurno,
        eventi: mpsc::Sender<EventoTurno>,
    ) -> Result<ManigliaTurno, Errore> {
        let cartella = self.cartella_di(&richiesta);
        let impronta = firma(&self.programma, &cartella, &richiesta);
        let (stop_tx, stop_rx) = oneshot::channel();
        let lavoro = caldi::Lavoro {
            messaggio: messaggio_utente(&richiesta),
            eventi,
            stop: stop_rx,
        };
        let lavoro = match caldi::consegna(&richiesta.sessione_id, ID, impronta, lavoro) {
            Ok(()) => {
                return Ok(caldi::maniglia(
                    caldi::pid_di(&richiesta.sessione_id, ID),
                    stop_tx,
                ));
            }
            Err(l) => l,
        };
        tokio::fs::create_dir_all(&cartella).await.map_err(|e| {
            Errore::interno(format!("cartella di lavoro {}: {e}", cartella.display()))
        })?;
        let numero = caldi::nuovo_numero();
        let mut file = Vec::new();
        if let Some(m) = richiesta.mcp.as_mut() {
            // Un file per processo: vive quanto lui (un processo nuovo non cancella quello vecchio).
            m.file = m.file.with_file_name(format!("mcp-config-{numero}.json"));
            mcp::scrivi(m).await?;
            file.push(m.file.clone());
        }
        let figlio = match lancia(&self.programma, &cartella, &richiesta) {
            Ok(f) => f,
            Err(e) => {
                file.iter().for_each(|f| caldi::cancella_file(f));
                return Err(e);
            }
        };
        let pid = figlio.id();
        let processo = Processo::nuovo(figlio, self.silenzio, self.attesa_kill)?;
        caldi::apri(
            &richiesta.sessione_id,
            ID,
            impronta,
            numero,
            pid,
            file,
            processo,
            lavoro,
        );
        Ok(caldi::maniglia(pid, stop_tx))
    }
}

fn lancia(programma: &Path, cartella: &Path, r: &RichiestaTurno) -> Result<Child, Errore> {
    let mut cmd = Command::new(programma);
    cmd.args(argomenti(r))
        .current_dir(cartella)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for k in AMBIENTE_DA_TOGLIERE {
        cmd.env_remove(k);
    }
    // Niente `XBOT_*` del server né chiavi ereditate; poi i segreti dell'assistente, quelli
    // approvati per il turno e le variabili del computer: solo nell'ambiente del processo.
    crate::esterni::metti_ambiente(&mut cmd, &r.ambiente);
    let figlio = cmd.spawn().map_err(|e| {
        Errore::nuovo(
            Codice::ProviderFermo,
            format!("claude non parte ({}): {e}", programma.display()),
        )
    })?;
    tracing::info!(pid = ?figlio.id(), resume = r.sessione_esterna_id.is_some(), "claude avviato");
    Ok(figlio)
}

#[cfg(test)]
mod prove;
