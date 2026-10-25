//! Fase 2.H: il computer di ogni assistente. Con il computer acceso il server tiene sulla stessa
//! macchina un desktop dell'assistente: Xvfb `:N`, un gestore di finestre se c'è (openbox o
//! fluxbox), x11vnc solo su 127.0.0.1, Chrome col suo profilo e la porta di debug, ttyd con tmux.
//! Lo schermo arriva al browser dal server (proxy WebSocket → x11vnc), il terminale da ttyd
//! (proxy HTTP e WebSocket con `--base-path`): nessuna porta esce da 127.0.0.1.
//!
//! I processi partono in gruppi di processi propri, con un ambiente pulito (nessun segreto del
//! server). Uno che muore porta il computer in `errore` con il testo; `ricrea` lo rifà. Alla
//! chiusura del server tutto si spegne; all'avvio ripartono quelli con `computer_acceso = 1`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, watch};
use xbot_core::{Errore, Risposta};
use xbot_db::Db;
use xbot_db::computer::{self as repo, Posto};

/// Larghezza e altezza dello schermo virtuale.
pub const LARGHEZZA: i64 = 1280;
pub const ALTEZZA: i64 = 800;

/// I programmi del desktop: ognuno è il comando con i suoi primi argomenti (nelle prove uno
/// script finto con il ruolo davanti).
#[derive(Debug, Clone)]
pub struct Programmi {
    pub xvfb: Option<Vec<String>>,
    pub wm: Option<Vec<String>>,
    pub x11vnc: Option<Vec<String>>,
    pub chrome: Option<Vec<String>>,
    pub ttyd: Option<Vec<String>>,
    /// `tmux` per chiudere la sessione del terminale (tmux si stacca da ttyd).
    pub tmux: Option<String>,
    /// La cartella dei socket X (`/tmp/.X11-unix`): Xvfb è pronto quando c'è `X<N>`.
    pub cartella_x11: PathBuf,
    /// Variabili in più per i processi (nelle prove: dove il finto mette i socket).
    pub ambiente: Vec<(String, String)>,
    /// Quanto aspettare che un processo sia pronto.
    pub attesa: Duration,
}

/// Il percorso di un programma nel PATH, se c'è.
fn trova(nome: &str) -> Option<String> {
    if nome.contains('/') {
        return Path::new(nome).is_file().then(|| nome.to_string());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(nome))
        .find(|p| p.is_file())
        .map(|p| p.to_string_lossy().into_owned())
}

impl Programmi {
    /// Quelli che ci sono su questa macchina.
    pub fn di_sistema() -> Self {
        let uno = |n: &str| trova(n).map(|p| vec![p]);
        Self {
            xvfb: uno("Xvfb"),
            wm: uno("openbox").or_else(|| uno("fluxbox")),
            x11vnc: uno("x11vnc"),
            chrome: uno("google-chrome")
                .or_else(|| uno("google-chrome-stable"))
                .or_else(|| uno("chromium"))
                .or_else(|| uno("chromium-browser")),
            ttyd: uno("ttyd"),
            tmux: trova("tmux"),
            cartella_x11: PathBuf::from("/tmp/.X11-unix"),
            ambiente: Vec::new(),
            attesa: Duration::from_secs(15),
        }
    }

    /// Per le prove: tutti i ruoli fatti da `script` (vedi `tests/desktop-finto.py`).
    pub fn finti(script: &Path, cartella_x11: &Path) -> Self {
        let r = |ruolo: &str| {
            Some(vec![
                "python3".into(),
                script.to_string_lossy().into_owned(),
                ruolo.into(),
            ])
        };
        Self {
            xvfb: r("xvfb"),
            wm: r("wm"),
            x11vnc: r("x11vnc"),
            chrome: r("chrome"),
            ttyd: r("ttyd"),
            tmux: None,
            cartella_x11: cartella_x11.to_path_buf(),
            ambiente: vec![(
                "FINTO_X11".into(),
                cartella_x11.to_string_lossy().into_owned(),
            )],
            attesa: Duration::from_secs(10),
        }
    }

    fn mancanti(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        for (n, p) in [
            ("Xvfb", &self.xvfb),
            ("x11vnc", &self.x11vnc),
            ("Chrome", &self.chrome),
            ("ttyd", &self.ttyd),
        ] {
            if p.is_none() {
                v.push(n);
            }
        }
        v
    }
}

/// Un computer acceso: il posto e il modo di spegnerlo.
struct Vivo {
    posto: Posto,
    ferma: watch::Sender<bool>,
    finito: Option<tokio::task::JoinHandle<()>>,
}

struct Interno {
    db: Db,
    cartella: Mutex<PathBuf>,
    programmi: Mutex<Programmi>,
    vivi: Mutex<HashMap<String, Vivo>>,
    /// Una sola accensione o spegnimento alla volta.
    operazioni: tokio::sync::Mutex<()>,
    /// Il server si sta chiudendo: un processo che muore non è un guasto e non si riprende.
    in_chiusura: AtomicBool,
    http: reqwest::Client,
}

/// I computer degli assistenti di questo server.
#[derive(Clone)]
pub struct Computer(Arc<Interno>);

fn blocca<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    }
}

/// Il percorso base del terminale di un assistente (anche `--base-path` di ttyd).
pub fn base_terminale(id: &str) -> String {
    format!("/api/v1/assistenti/{id}/computer/terminale")
}

/// L'id di un assistente usato nei nomi (cartelle, tmux): solo lettere, cifre, `-` e `_`.
fn id_sicuro(id: &str) -> Risposta<&str> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(Errore::non_valida("id dell'assistente non valido"));
    }
    Ok(id)
}

fn porta_libera(p: i64) -> bool {
    u16::try_from(p).is_ok_and(|p| std::net::TcpListener::bind(("127.0.0.1", p)).is_ok())
}

/// Siamo root? (Chrome da root vuole `--no-sandbox`.)
fn da_root() -> bool {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0)
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Manda un segnale a tutto il gruppo di processi (`kill -SEGNALE -- -pgid`).
async fn segnale_gruppo(pid: u32, segnale: &str) {
    let _ = Command::new("kill")
        .arg(format!("-{segnale}"))
        .arg("--")
        .arg(format!("-{pid}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
}

/// Un processo del desktop, con il suo ruolo.
struct Processo {
    ruolo: &'static str,
    figlio: Child,
}

impl Computer {
    pub fn nuovo(db: Db, cartella: PathBuf) -> Self {
        Self(Arc::new(Interno {
            db,
            cartella: Mutex::new(cartella),
            programmi: Mutex::new(Programmi::di_sistema()),
            vivi: Mutex::new(HashMap::new()),
            operazioni: tokio::sync::Mutex::new(()),
            in_chiusura: AtomicBool::new(false),
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap_or_default(),
        }))
    }

    /// Cambia i programmi (le prove usano quelli finti).
    pub fn imposta_programmi(&self, p: Programmi) {
        *blocca(&self.0.programmi) = p;
    }

    /// Cambia la cartella dei dati dei computer (`XBOT_DATI/computer`).
    pub fn imposta_cartella(&self, c: PathBuf) {
        *blocca(&self.0.cartella) = c;
    }

    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.0.http
    }

    /// La casa del computer dell'assistente: `XBOT_DATI/computer/<id>/` (0700).
    pub fn casa(&self, id: &str) -> PathBuf {
        blocca(&self.0.cartella).join(id)
    }

    /// Il posto del computer acceso (display e porte), o `None`.
    pub fn posto(&self, id: &str) -> Option<Posto> {
        blocca(&self.0.vivi).get(id).map(|v| v.posto)
    }

    /// Lo stato come lo vede la scheda Computer.
    pub async fn info(&self, id: &str) -> Risposta<Value> {
        let aid = id.to_string();
        let r = self.0.db.esegui(move |c| repo::leggi(c, &aid)).await?;
        Ok(json!({
            "stato": r["stato"], "display": r["display"],
            "larghezza": r["larghezza"], "altezza": r["altezza"],
            "acceso_ms": r["acceso_ms"], "errore": r["errore"],
            "schermo": format!("/api/v1/assistenti/{id}/computer/schermo"),
            "terminale": format!("{}/", base_terminale(id)),
        }))
    }

    /// Accende (idempotente): se è già acceso o in avvio non fa niente.
    pub async fn accendi(&self, id: &str) -> Risposta<Value> {
        id_sicuro(id)?;
        let _g = self.0.operazioni.lock().await;
        self.accendi_dentro(id).await?;
        self.info(id).await
    }

    /// Spegne e ricorda che l'utente lo vuole spento.
    pub async fn spegni(&self, id: &str) -> Risposta<Value> {
        id_sicuro(id)?;
        let _g = self.0.operazioni.lock().await;
        let aid = id.to_string();
        self.0
            .db
            .esegui(move |c| repo::imposta_acceso(c, &aid, false))
            .await?;
        self.ferma(id).await;
        let aid = id.to_string();
        self.0.db.esegui(move |c| repo::libera(c, &aid)).await?;
        self.info(id).await
    }

    /// Spegne (se serve) e riaccende da capo.
    pub async fn ricrea(&self, id: &str) -> Risposta<Value> {
        id_sicuro(id)?;
        let _g = self.0.operazioni.lock().await;
        self.ferma(id).await;
        self.accendi_dentro(id).await?;
        self.info(id).await
    }

    /// Alla chiusura del server: spegne tutti, senza cambiare la volontà dell'utente.
    pub async fn spegni_tutti(&self) {
        self.0.in_chiusura.store(true, Ordering::SeqCst);
        let ids: Vec<String> = blocca(&self.0.vivi).keys().cloned().collect();
        // In parallelo: systemd aspetta 10 s in tutto, non per ogni computer.
        let compiti = ids.into_iter().map(|id| async move {
            self.ferma(&id).await;
            let _ = self.0.db.esegui(move |c| repo::libera(c, &id)).await;
        });
        futures::future::join_all(compiti).await;
    }

    /// All'avvio del server: le righe rimaste di prima si azzerano, poi ripartono i computer
    /// che l'utente vuole accesi.
    pub async fn riavvia_accesi(&self) {
        let r = self
            .0
            .db
            .esegui(|c| {
                c.execute(
                    "UPDATE computer_assistente SET stato = 'spento', display = NULL,
                       porta_vnc = NULL, porta_ws = NULL, porta_ttyd = NULL,
                       porta_chrome = NULL, acceso_ms = NULL WHERE stato != 'spento'",
                    [],
                )
                .map_err(|e| Errore::interno(format!("computer: {e}")))?;
                repo::da_accendere(c)
            })
            .await;
        match r {
            Ok(ids) => {
                for id in ids {
                    match self.accendi(&id).await {
                        Ok(v) => {
                            tracing::info!(assistente = %id, stato = %v["stato"], "computer riacceso")
                        }
                        Err(e) => {
                            tracing::warn!(assistente = %id, errore = %e, "computer non riacceso")
                        }
                    }
                }
            }
            Err(e) => tracing::warn!(errore = %e, "computer da riaccendere non letti"),
        }
    }

    async fn ferma(&self, id: &str) {
        let vivo = blocca(&self.0.vivi).remove(id);
        if let Some(mut v) = vivo {
            v.ferma.send_replace(true);
            if let Some(h) = v.finito.take() {
                let _ = tokio::time::timeout(Duration::from_secs(6), h).await;
            }
        }
    }

    async fn accendi_dentro(&self, id: &str) -> Risposta<()> {
        self.accendi_tentativo(id, 0).await
    }

    /// `tentativo`: quante riprese di fila ha già fatto questo computer dopo un guasto.
    fn accendi_tentativo<'a>(
        &'a self,
        id: &'a str,
        tentativo: u32,
    ) -> futures::future::BoxFuture<'a, Risposta<()>> {
        Box::pin(self.accendi_corpo(id, tentativo))
    }

    async fn accendi_corpo(&self, id: &str, tentativo: u32) -> Risposta<()> {
        if blocca(&self.0.vivi).contains_key(id) {
            return Ok(());
        }
        let programmi = blocca(&self.0.programmi).clone();
        let mancanti = programmi.mancanti();
        if !mancanti.is_empty() {
            return Err(Errore::non_valida(format!(
                "su questa macchina mancano {}: il computer non si accende",
                mancanti.join(", ")
            )));
        }
        let (aid, x11) = (id.to_string(), programmi.cartella_x11.clone());
        let posto = self
            .0
            .db
            .esegui(move |c| {
                repo::imposta_acceso(c, &aid, true)?;
                let p = repo::assegna_posto(c, &aid, |p| {
                    !x11.join(format!("X{}", p.display)).exists()
                        && porta_libera(p.porta_vnc)
                        && porta_libera(p.porta_ttyd)
                        && porta_libera(p.porta_chrome)
                })?;
                match p {
                    Some(p) => {
                        repo::scrivi(c, &aid, "avvio", Some(&p), None)?;
                        Ok(Some(p))
                    }
                    None => {
                        repo::scrivi(c, &aid, "errore", None, Some("nessun posto libero"))?;
                        Ok(None)
                    }
                }
            })
            .await?;
        let Some(posto) = posto else {
            return Ok(());
        };
        let casa = self.casa(id);
        let cartella_lavoro = self.cartella_lavoro(id, &casa).await;
        match avvia_processi(id, &programmi, &posto, &casa, &cartella_lavoro).await {
            Ok(processi) => {
                let (ferma, rx) = watch::channel(false);
                let (me, aid) = (self.clone(), id.to_string());
                let tmux = programmi.tmux.clone();
                let casa2 = casa.clone();
                let finito = tokio::spawn(async move {
                    let partito = tokio::time::Instant::now();
                    let motivo = sorveglia(processi, rx).await;
                    chiudi_tmux(tmux.as_deref(), &casa2, &aid).await;
                    let Some(m) = motivo else { return };
                    if me.0.in_chiusura.load(Ordering::SeqCst) {
                        // Il server si spegne e systemd ha già fermato i figli: non è un guasto.
                        blocca(&me.0.vivi).remove(&aid);
                        return;
                    }
                    tracing::warn!(assistente = %aid, motivo = %m, "computer in errore");
                    blocca(&me.0.vivi).remove(&aid);
                    let (a2, m2) = (aid.clone(), m.clone());
                    let _ =
                        me.0.db
                            .esegui(move |c| repo::scrivi(c, &a2, "errore", None, Some(&m2)))
                            .await;
                    // Ripresa automatica: fino a 3 tentativi di fila (5, 15, 45 s); un computer
                    // che ha retto più di 5 minuti riparte dal primo.
                    let tentativo = if partito.elapsed() > Duration::from_secs(300) {
                        0
                    } else {
                        tentativo
                    };
                    if tentativo >= 3 {
                        tracing::warn!(assistente = %aid, "computer: riprese finite, resta in errore");
                        return;
                    }
                    let attesa = Duration::from_secs(5 * 3u64.pow(tentativo));
                    tokio::time::sleep(attesa).await;
                    let _g = me.0.operazioni.lock().await;
                    let ancora_voluto = {
                        let a3 = aid.clone();
                        me.0.db
                            .esegui(repo::da_accendere)
                            .await
                            .is_ok_and(|v| v.contains(&a3))
                    };
                    if ancora_voluto && !me.0.in_chiusura.load(Ordering::SeqCst) {
                        tracing::info!(assistente = %aid, tentativo = tentativo + 1, "computer: ripresa");
                        if let Err(e) = me.accendi_tentativo(&aid, tentativo + 1).await {
                            tracing::warn!(assistente = %aid, errore = %e, "computer: ripresa fallita");
                        }
                    }
                });
                blocca(&self.0.vivi).insert(
                    id.to_string(),
                    Vivo {
                        posto,
                        ferma,
                        finito: Some(finito),
                    },
                );
                let aid = id.to_string();
                self.0
                    .db
                    .esegui(move |c| repo::scrivi(c, &aid, "acceso", None, None))
                    .await?;
                tracing::info!(assistente = %id, display = posto.display, "computer acceso");
            }
            Err(testo) => {
                chiudi_tmux(programmi.tmux.as_deref(), &casa, id).await;
                tracing::warn!(assistente = %id, errore = %testo, "computer non acceso");
                let aid = id.to_string();
                self.0
                    .db
                    .esegui(move |c| repo::scrivi(c, &aid, "errore", None, Some(&testo)))
                    .await?;
            }
        }
        Ok(())
    }

    /// La cartella del terminale: quella dell'ultima sessione dell'assistente, o la casa.
    async fn cartella_lavoro(&self, id: &str, casa: &Path) -> PathBuf {
        let aid = id.to_string();
        let c = self
            .0
            .db
            .esegui(move |c| {
                Ok(xbot_db::una_json(
                    c,
                    "SELECT cartella_lavoro FROM sessione WHERE assistente_id = ?1
                       AND cartella_lavoro IS NOT NULL ORDER BY aggiornata_ms DESC LIMIT 1",
                    [&aid],
                )?
                .and_then(|v| v["cartella_lavoro"].as_str().map(PathBuf::from)))
            })
            .await
            .ok()
            .flatten();
        c.filter(|p| p.is_dir())
            .unwrap_or_else(|| casa.to_path_buf())
    }

    /// Per i turni: `(DISPLAY, casa, porta di debug di Chrome)` se il computer è acceso.
    pub fn per_turno(&self, id: &str) -> Option<(String, PathBuf, i64)> {
        self.posto(id)
            .map(|p| (format!(":{}", p.display), self.casa(id), p.porta_chrome))
    }
}

/// Crea la casa (0700) con il profilo di Chrome.
fn prepara_casa(casa: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(casa.join("chrome"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(casa, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// L'ambiente pulito dei processi del desktop: nessun segreto del server passa.
fn ambiente(p: &Programmi, display: &str, casa: &Path) -> Vec<(String, String)> {
    let mut v = vec![
        (
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into()),
        ),
        ("LANG".into(), "C.UTF-8".into()),
        ("HOME".into(), casa.to_string_lossy().into_owned()),
        ("DISPLAY".into(), display.into()),
        ("TMUX_TMPDIR".into(), casa.to_string_lossy().into_owned()),
        ("TERM".into(), "xterm-256color".into()),
    ];
    v.extend(p.ambiente.iter().cloned());
    v
}

fn lancia(
    ruolo: &'static str,
    comando: &[String],
    argomenti: &[String],
    env: &[(String, String)],
    cartella: &Path,
    log: &Path,
) -> Result<Processo, String> {
    let (prog, primi) = comando
        .split_first()
        .ok_or_else(|| format!("{ruolo}: comando vuoto"))?;
    let uscita = std::fs::File::options()
        .create(true)
        .append(true)
        .open(log)
        .map_err(|e| format!("{ruolo}: registro {}: {e}", log.display()))?;
    let errori = uscita
        .try_clone()
        .map_err(|e| format!("{ruolo}: registro: {e}"))?;
    let mut c = Command::new(prog);
    c.args(primi)
        .args(argomenti)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .current_dir(cartella)
        .stdin(Stdio::null())
        .stdout(uscita)
        .stderr(errori)
        .kill_on_drop(true);
    #[cfg(unix)]
    c.process_group(0);
    let figlio = c.spawn().map_err(|e| format!("{ruolo} non parte: {e}"))?;
    Ok(Processo { ruolo, figlio })
}

/// Aspetta che `pronto` sia vero, finché il processo è vivo.
async fn aspetta(
    p: &mut Processo,
    attesa: Duration,
    pronto: impl Fn() -> bool,
) -> Result<(), String> {
    let fine = tokio::time::Instant::now() + attesa;
    loop {
        if pronto() {
            return Ok(());
        }
        if let Ok(Some(st)) = p.figlio.try_wait() {
            return Err(format!("{} si è fermato all'avvio ({st})", p.ruolo));
        }
        if tokio::time::Instant::now() > fine {
            return Err(format!("{} non è pronto dopo {:?}", p.ruolo, attesa));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn porta_aperta(p: i64) -> bool {
    u16::try_from(p).is_ok_and(|p| {
        std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], p)),
            Duration::from_millis(200),
        )
        .is_ok()
    })
}

async fn termina(processi: &mut [Processo]) {
    for p in processi.iter().rev() {
        if let Some(pid) = p.figlio.id() {
            segnale_gruppo(pid, "TERM").await;
        }
    }
    let fine = tokio::time::Instant::now() + Duration::from_secs(3);
    for p in processi.iter_mut() {
        let resto = fine.saturating_duration_since(tokio::time::Instant::now());
        if tokio::time::timeout(resto, p.figlio.wait()).await.is_err() {
            if let Some(pid) = p.figlio.id() {
                segnale_gruppo(pid, "KILL").await;
            }
            let _ = p.figlio.kill().await;
        }
    }
}

async fn chiudi_tmux(tmux: Option<&str>, casa: &Path, id: &str) {
    let Some(t) = tmux else { return };
    let _ = Command::new(t)
        .args(["kill-session", "-t", &format!("xbot-{id}")])
        .env("TMUX_TMPDIR", casa)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
}

/// Avvia i processi in ordine, ognuno solo quando il precedente è pronto.
async fn avvia_processi(
    id: &str,
    p: &Programmi,
    posto: &Posto,
    casa: &Path,
    cartella_lavoro: &Path,
) -> Result<Vec<Processo>, String> {
    prepara_casa(casa).map_err(|e| format!("casa {}: {e}", casa.display()))?;
    let display = format!(":{}", posto.display);
    let env = ambiente(p, &display, casa);
    let log = casa.join("desktop.log");
    let mut processi: Vec<Processo> = Vec::new();
    let r = async {
        let s = |v: &[&str]| v.iter().map(|x| (*x).to_string()).collect::<Vec<_>>();
        let geometria = format!("{LARGHEZZA}x{ALTEZZA}x24");
        let mut x = lancia(
            "Xvfb",
            p.xvfb.as_deref().unwrap_or_default(),
            &s(&[&display, "-screen", "0", &geometria, "-nolisten", "tcp"]),
            &env,
            casa,
            &log,
        )?;
        let sock = p.cartella_x11.join(format!("X{}", posto.display));
        let pronto = aspetta(&mut x, p.attesa, || sock.exists()).await;
        processi.push(x);
        pronto?;
        if let Some(wm) = &p.wm {
            processi.push(lancia("gestore di finestre", wm, &[], &env, casa, &log)?);
        }
        let vnc = posto.porta_vnc.to_string();
        let mut v = lancia(
            "x11vnc",
            p.x11vnc.as_deref().unwrap_or_default(),
            &s(&[
                "-display",
                &display,
                "-rfbport",
                &vnc,
                "-localhost",
                "-forever",
                "-shared",
                "-nopw",
                "-quiet",
            ]),
            &env,
            casa,
            &log,
        )?;
        let pronto = aspetta(&mut v, p.attesa, || porta_aperta(posto.porta_vnc)).await;
        processi.push(v);
        pronto?;
        let mut a_chrome = vec![
            format!("--remote-debugging-port={}", posto.porta_chrome),
            format!("--user-data-dir={}", casa.join("chrome").display()),
            "--no-first-run".into(),
            "--no-default-browser-check".into(),
            "--disable-dev-shm-usage".into(),
            format!("--window-size={LARGHEZZA},{ALTEZZA}"),
            "--window-position=0,0".into(),
            "--start-maximized".into(),
        ];
        if da_root() {
            a_chrome.push("--no-sandbox".into());
        }
        a_chrome.push("about:blank".into());
        let mut c = lancia(
            "Chrome",
            p.chrome.as_deref().unwrap_or_default(),
            &a_chrome,
            &env,
            casa,
            &log,
        )?;
        let pronto = aspetta(&mut c, p.attesa, || porta_aperta(posto.porta_chrome)).await;
        processi.push(c);
        pronto?;
        let (porta_t, base, cwd) = (
            posto.porta_ttyd.to_string(),
            base_terminale(id),
            cartella_lavoro.to_string_lossy().into_owned(),
        );
        let sessione = format!("xbot-{id}");
        let mut t = lancia(
            "ttyd",
            p.ttyd.as_deref().unwrap_or_default(),
            &s(&[
                "-i",
                "127.0.0.1",
                "-p",
                &porta_t,
                "-W",
                "-b",
                &base,
                "-w",
                &cwd,
                "tmux",
                "new",
                "-A",
                "-s",
                &sessione,
            ]),
            &env,
            casa,
            &log,
        )?;
        let pronto = aspetta(&mut t, p.attesa, || porta_aperta(posto.porta_ttyd)).await;
        processi.push(t);
        pronto?;
        Ok::<(), String>(())
    }
    .await;
    match r {
        Ok(()) => Ok(processi),
        Err(e) => {
            termina(&mut processi).await;
            Err(e)
        }
    }
}

/// Sorveglia i processi: al primo che muore (o alla richiesta di spegnere) chiude gli altri.
/// Restituisce il motivo dell'errore, `None` se lo spegnimento era voluto.
async fn sorveglia(
    mut processi: Vec<Processo>,
    mut ferma: watch::Receiver<bool>,
) -> Option<String> {
    let (tx, mut rx) = mpsc::channel::<String>(processi.len().max(1));
    // Ogni processo ha il suo sorvegliante; la richiesta di spegnere passa da `ferma`.
    let (stop_tx, _) = watch::channel(false);
    let mut compiti = Vec::new();
    for mut p in processi.drain(..) {
        let tx = tx.clone();
        let mut stop = stop_tx.subscribe();
        compiti.push(tokio::spawn(async move {
            tokio::select! {
                st = p.figlio.wait() => {
                    let st = st.map_or_else(|e| e.to_string(), |s| s.to_string());
                    let _ = tx.send(format!("{} si è fermato ({st})", p.ruolo)).await;
                    if let Some(pid) = p.figlio.id() { segnale_gruppo(pid, "TERM").await; }
                }
                _ = stop.changed() => {
                    termina(std::slice::from_mut(&mut p)).await;
                }
            }
        }));
    }
    drop(tx);
    let motivo = if *ferma.borrow() {
        None
    } else {
        tokio::select! {
            m = rx.recv() => m,
            _ = ferma.changed() => None,
        }
    };
    stop_tx.send_replace(true);
    for c in compiti {
        let _ = c.await;
    }
    motivo
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn id_e_base() {
        assert!(id_sicuro("01ABC_x-1").is_ok());
        assert!(id_sicuro("../x").is_err());
        assert!(id_sicuro("").is_err());
        assert_eq!(
            base_terminale("A1"),
            "/api/v1/assistenti/A1/computer/terminale"
        );
        assert!(base_terminale(&"A".repeat(26)).len() <= 128);
    }

    #[test]
    fn ambiente_senza_segreti() {
        let p = Programmi::finti(Path::new("/x.py"), Path::new("/tmp/x"));
        let env = ambiente(&p, ":50", Path::new("/casa"));
        let nomi: Vec<&str> = env.iter().map(|(k, _)| k.as_str()).collect();
        assert!(nomi.contains(&"DISPLAY") && nomi.contains(&"HOME"));
        assert!(!nomi.iter().any(|n| n.starts_with("XBOT_")));
    }
}
