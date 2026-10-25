//! I processi caldi (Fase 2.G): un processo del client per sessione, tenuto vivo fra i turni.
//!
//! La cassetta è unica per tutto il processo (server o lavoratore): la chiave è la sessione più
//! il provider; la «firma» (programma, modello, livello, permessi, cartella, ambiente, MCP) dice
//! se il processo caldo va ancora bene. Una firma diversa chiude il vecchio e ne apre uno nuovo.
//! Ogni processo ha un custode (un compito tokio) che riceve i turni uno alla volta; dopo
//! `XBOT_CALDO_MINUTI` minuti senza turni (15 di norma, 0 = spento: il processo si chiude a fine
//! turno) il custode chiude il processo. Un processo morto fra due turni esce dalla cassetta da
//! solo: il turno dopo ne apre uno nuovo.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};

use crate::{EventoTurno, ManigliaTurno};

/// Variabile dei minuti di inattività prima di chiudere un processo caldo.
pub const VARIABILE_MINUTI: &str = "XBOT_CALDO_MINUTI";
const MINUTI_PREDEFINITI: u64 = 15;

/// Quanto aspetta un processo caldo senza turni (0 = si chiude a fine turno).
pub fn inattivita() -> Duration {
    let minuti = std::env::var(VARIABILE_MINUTI)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(MINUTI_PREDEFINITI);
    Duration::from_secs(minuti.saturating_mul(60))
}

/// Vero se i processi caldi sono accesi (`XBOT_CALDO_MINUTI` diverso da 0).
pub fn accesi() -> bool {
    !inattivita().is_zero()
}

/// Un'impronta di quello che rende un processo diverso da un altro (i segreti entrano solo
/// come impronta).
#[derive(Debug, Default)]
pub struct Firma(std::collections::hash_map::DefaultHasher);

impl Firma {
    pub fn con(mut self, v: impl Hash) -> Self {
        v.hash(&mut self.0);
        self
    }

    pub fn fine(&self) -> u64 {
        self.0.finish()
    }
}

/// Come è finito un turno sul processo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DopoTurno {
    /// Il processo può ricevere il turno dopo.
    Pronto,
    /// Il processo va chiuso (interrotto, morto, silenzioso, in errore).
    Chiudi,
}

/// Un turno consegnato al custode.
pub struct Lavoro<M> {
    pub messaggio: M,
    pub eventi: mpsc::Sender<EventoTurno>,
    pub stop: oneshot::Receiver<()>,
}

/// Il dialogo con un processo vivo: un turno alla volta.
#[async_trait]
pub trait Conversazione: Send + 'static {
    type Messaggio: Send + 'static;
    /// Esegue un turno: manda gli eventi e dice se il processo resta buono.
    async fn turno(&mut self, l: Lavoro<Self::Messaggio>) -> DopoTurno;
    /// Aspetta che il processo esca da solo (per il custode fra i turni).
    async fn uscito(&mut self);
    /// Chiude il processo (SIGTERM, poi SIGKILL) e pulisce i suoi file.
    async fn chiudi(&mut self);
}

struct Voce {
    id: u64,
    firma: u64,
    /// Il mittente dei turni, cancellato con tipo (`Box<dyn Any>`) perché la cassetta è una sola.
    turni: Box<dyn std::any::Any + Send>,
}

#[derive(Default)]
struct Registro {
    voci: HashMap<(String, String), Voce>,
    /// id → (pid, file da cancellare): per chiudere tutto a SIGTERM.
    vivi: HashMap<u64, (Option<u32>, Vec<PathBuf>)>,
}

fn registro() -> MutexGuard<'static, Registro> {
    static R: OnceLock<Mutex<Registro>> = OnceLock::new();
    let m = R.get_or_init(Mutex::default);
    match m.lock() {
        Ok(g) => g,
        Err(a) => a.into_inner(),
    }
}

static PROSSIMO: AtomicU64 = AtomicU64::new(1);

/// Un numero nuovo per un processo (anche per i nomi dei suoi file).
pub fn nuovo_numero() -> u64 {
    PROSSIMO.fetch_add(1, Ordering::Relaxed)
}

/// Prova a consegnare il turno al processo caldo con quella firma. Se non c'è (o la firma è
/// cambiata, o è morto) restituisce il lavoro: chi chiama apre un processo nuovo con `apri`.
pub fn consegna<M: Send + 'static>(
    sessione: &str,
    provider: &str,
    firma: u64,
    lavoro: Lavoro<M>,
) -> Result<(), Lavoro<M>> {
    let chiave = (sessione.to_string(), provider.to_string());
    let mut r = registro();
    let Some(v) = r.voci.get(&chiave) else {
        return Err(lavoro);
    };
    if v.firma != firma {
        // Il vecchio processo chiude da solo quando il suo mittente cade.
        tracing::info!(
            sessione,
            provider,
            "processo caldo con firma diversa: si riapre"
        );
        r.voci.remove(&chiave);
        return Err(lavoro);
    }
    let Some(tx) = v.turni.downcast_ref::<mpsc::Sender<Lavoro<M>>>() else {
        r.voci.remove(&chiave);
        return Err(lavoro);
    };
    match tx.try_send(lavoro) {
        Ok(()) => {
            tracing::info!(sessione, provider, "turno sul processo caldo");
            Ok(())
        }
        Err(mpsc::error::TrySendError::Full(l) | mpsc::error::TrySendError::Closed(l)) => {
            r.voci.remove(&chiave);
            Err(l)
        }
    }
}

/// Mette in cassetta un processo appena aperto e gli consegna il primo turno.
/// `file` sono i file da cancellare quando il processo si chiude (es. il `--mcp-config`).
#[allow(clippy::too_many_arguments)]
pub fn apri<C: Conversazione>(
    sessione: &str,
    provider: &str,
    firma: u64,
    numero: u64,
    pid: Option<u32>,
    file: Vec<PathBuf>,
    conversazione: C,
    lavoro: Lavoro<C::Messaggio>,
) {
    let (tx, rx) = mpsc::channel::<Lavoro<C::Messaggio>>(1);
    let attesa = inattivita();
    let chiave = (sessione.to_string(), provider.to_string());
    {
        let mut r = registro();
        r.vivi.insert(numero, (pid, file.clone()));
        if !attesa.is_zero() {
            r.voci.insert(
                chiave.clone(),
                Voce {
                    id: numero,
                    firma,
                    turni: Box::new(tx.clone()),
                },
            );
        }
    }
    // Il primo turno entra subito: il canale ha posto per uno.
    if tx.try_send(lavoro).is_err() {
        tracing::error!("primo turno del processo non consegnato");
    }
    drop(tx);
    tokio::spawn(custode(chiave, numero, file, conversazione, rx, attesa));
}

async fn custode<C: Conversazione>(
    chiave: (String, String),
    numero: u64,
    file: Vec<PathBuf>,
    mut c: C,
    mut rx: mpsc::Receiver<Lavoro<C::Messaggio>>,
    attesa: Duration,
) {
    let mut primo = true;
    loop {
        let lavoro = if primo {
            primo = false;
            rx.recv().await
        } else if attesa.is_zero() {
            None
        } else {
            tokio::select! {
                l = rx.recv() => l,
                () = c.uscito() => {
                    tracing::info!(sessione = %chiave.0, "processo caldo uscito da solo");
                    None
                }
                () = tokio::time::sleep(attesa) => {
                    tracing::info!(sessione = %chiave.0, "processo caldo inattivo: chiuso");
                    None
                }
            }
        };
        let Some(l) = lavoro else { break };
        if c.turno(l).await == DopoTurno::Chiudi {
            break;
        }
    }
    {
        let mut r = registro();
        if r.voci.get(&chiave).is_some_and(|v| v.id == numero) {
            r.voci.remove(&chiave);
        }
        r.vivi.remove(&numero);
    }
    rx.close();
    c.chiudi().await;
    for f in file {
        cancella_file(&f);
    }
}

/// Cancella un file del processo e la sua cartella se resta vuota.
pub fn cancella_file(f: &std::path::Path) {
    match std::fs::remove_file(f) {
        Ok(()) => {
            if let Some(d) = f.parent() {
                let _ = std::fs::remove_dir(d);
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            tracing::warn!(file = %f.display(), errore = %e, "file del processo non cancellato")
        }
    }
}

/// Toglie dalla cassetta il processo caldo di una sessione (lo chiude il suo custode).
pub fn dimentica(sessione: &str, provider: &str) {
    registro()
        .voci
        .remove(&(sessione.to_string(), provider.to_string()));
}

/// Quanti processi caldi sono vivi (per le prove e i registri).
pub fn quanti() -> usize {
    registro().vivi.len()
}

/// Chiude tutti i processi caldi: SIGTERM, mezzo secondo, SIGKILL; cancella i loro file.
/// Si chiama a SIGTERM del server o del lavoratore, prima di uscire (è sincrona).
pub fn chiudi_tutti() {
    let vivi: Vec<(Option<u32>, Vec<PathBuf>)> = {
        let mut r = registro();
        r.voci.clear();
        r.vivi.drain().map(|(_, v)| v).collect()
    };
    if vivi.is_empty() {
        return;
    }
    tracing::info!(processi = vivi.len(), "chiusura dei processi caldi");
    for (pid, _) in &vivi {
        segnale(*pid, nix::sys::signal::Signal::SIGTERM);
    }
    std::thread::sleep(Duration::from_millis(500));
    for (pid, file) in &vivi {
        segnale(*pid, nix::sys::signal::Signal::SIGKILL);
        for f in file {
            cancella_file(f);
        }
    }
}

pub(crate) fn segnale(pid: Option<u32>, s: nix::sys::signal::Signal) {
    if let Some(p) = pid.and_then(|p| i32::try_from(p).ok())
        && let Err(e) = nix::sys::signal::kill(nix::unistd::Pid::from_raw(p), s)
    {
        tracing::debug!(pid = p, errore = %e, "segnale non consegnato");
    }
}

/// SIGTERM, poi SIGKILL se dopo `attesa` il processo è ancora vivo.
pub(crate) async fn ferma(figlio: &mut tokio::process::Child, attesa: Duration) {
    if matches!(figlio.try_wait(), Ok(Some(_))) {
        return;
    }
    segnale(figlio.id(), nix::sys::signal::Signal::SIGTERM);
    if tokio::time::timeout(attesa, figlio.wait()).await.is_err() {
        tracing::warn!(pid = ?figlio.id(), "il client non si ferma: SIGKILL");
        if let Err(e) = figlio.kill().await {
            tracing::warn!(errore = %e, "SIGKILL fallito");
        }
    }
}

/// La maniglia di un turno consegnato: `interrompi` manda lo stop al custode.
pub fn maniglia(pid: Option<u32>, stop: oneshot::Sender<()>) -> ManigliaTurno {
    ManigliaTurno {
        pid,
        interrompi: Box::new(move || {
            let _ = stop.send(());
        }),
    }
}

/// Il pid del processo caldo di una sessione, se c'è (per la maniglia del turno).
pub fn pid_di(sessione: &str, provider: &str) -> Option<u32> {
    let r = registro();
    let v = r.voci.get(&(sessione.to_string(), provider.to_string()))?;
    r.vivi.get(&v.id).and_then(|(p, _)| *p)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn firma_cambia_con_i_campi() {
        let a = Firma::default().con("m").con(Some("low")).fine();
        let b = Firma::default().con("m").con(Some("high")).fine();
        assert_ne!(a, b);
        assert_eq!(a, Firma::default().con("m").con(Some("low")).fine());
    }
}
