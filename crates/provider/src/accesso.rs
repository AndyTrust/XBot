//! Fase 2.I: l'account dei client ufficiali e i loro comandi di accesso e uscita.
//!
//! - `claude auth status` (JSON): `email`, `subscriptionType`; `claude auth login` stampa
//!   «If the browser didn't open, visit: <url>» e poi «Paste code here if prompted >».
//! - `agent status --format json`: `isAuthenticated`, `userInfo.email`; `NO_OPEN_BROWSER=1 agent
//!   login` stampa «Open a browser and navigate to this link: <url>» e aspetta.
//! - `codex login status` (su stderr): «Logged in using ChatGPT» / «Not logged in»;
//!   `codex login --device-auth` stampa l'url e un codice da scrivere. Attenzione: `codex login`
//!   toglie subito l'accesso che c'era.
//! - grok: `$GROK_HOME/auth.json` (o `~/.grok/auth.json`), una voce per account con `email`;
//!   `grok login --device-auth` stampa l'url (con `user_code`) e il codice.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;
use xbot_core::Errore;

/// L'account con cui un client è entrato.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Account {
    pub nome: String,
    #[serde(default)]
    pub dettaglio: Option<String>,
}

/// Un comando di accesso o di uscita di un client.
#[derive(Debug, Clone, PartialEq)]
pub struct ComandoAccesso {
    pub programma: PathBuf,
    pub argomenti: Vec<String>,
    pub ambiente: Vec<(String, String)>,
    /// Il client chiede un codice da incollare (Claude Code senza ritorno automatico).
    pub chiede_codice: bool,
}

impl ComandoAccesso {
    pub fn nuovo(programma: &Path, argomenti: &[&str]) -> Self {
        Self {
            programma: programma.to_path_buf(),
            argomenti: argomenti.iter().map(|s| s.to_string()).collect(),
            ambiente: Vec::new(),
            chiede_codice: false,
        }
    }

    /// Il comando come si scrive nel Terminale.
    pub fn testo(&self) -> String {
        let mut v: Vec<String> = self
            .ambiente
            .iter()
            .map(|(k, val)| format!("{k}={val}"))
            .collect();
        v.push(self.programma.to_string_lossy().into_owned());
        v.extend(self.argomenti.iter().cloned());
        v.join(" ")
    }
}

/// Lancia un comando di sola lettura e restituisce stdout + stderr (20 s al massimo).
pub async fn uscita(programma: &Path, argomenti: &[&str]) -> Result<(bool, String), Errore> {
    let mut c = Command::new(programma);
    c.args(argomenti)
        .stdin(Stdio::null())
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .kill_on_drop(true);
    let o = tokio::time::timeout(Duration::from_secs(20), c.output())
        .await
        .map_err(|_| Errore::interno(format!("{}: troppo lento", programma.display())))?
        .map_err(|e| Errore::interno(format!("{} non parte: {e}", programma.display())))?;
    let mut t = String::from_utf8_lossy(&o.stdout).into_owned();
    t.push_str(&String::from_utf8_lossy(&o.stderr));
    Ok((o.status.success(), t))
}

/// `claude auth status` in JSON.
pub fn leggi_claude(testo: &str) -> Option<Account> {
    let v: Value = serde_json::from_str(testo.trim()).ok()?;
    if v["loggedIn"] != Value::Bool(true) {
        return None;
    }
    let nome = v["email"]
        .as_str()
        .or(v["orgName"].as_str())
        .unwrap_or("collegato");
    Some(Account {
        nome: nome.to_string(),
        dettaglio: v["subscriptionType"].as_str().map(str::to_string),
    })
}

/// `agent status --format json` (o il testo «Logged in as <email>»).
pub fn leggi_cursor(testo: &str) -> Option<Account> {
    if let Ok(v) = serde_json::from_str::<Value>(testo.trim()) {
        if v["isAuthenticated"] != Value::Bool(true) {
            return None;
        }
        let nome = v["userInfo"]["email"].as_str().unwrap_or("collegato");
        return Some(Account {
            nome: nome.to_string(),
            dettaglio: None,
        });
    }
    let riga = testo.lines().find(|l| l.contains("Logged in as"))?;
    let nome = riga.split("Logged in as").nth(1)?.trim();
    (!nome.is_empty()).then(|| Account {
        nome: nome.to_string(),
        dettaglio: None,
    })
}

/// `codex login status`: il client non dà l'email.
pub fn leggi_codex(testo: &str) -> Option<Account> {
    let riga = testo.lines().find(|l| l.contains("Logged in using"))?;
    let modo = riga.split("Logged in using").nth(1)?.trim();
    Some(Account {
        nome: if modo.is_empty() { "collegato" } else { modo }.to_string(),
        dettaglio: None,
    })
}

/// `auth.json` di grok: la prima voce con `email` (mai i token).
pub fn leggi_grok(testo: &str) -> Option<Account> {
    let v: Value = serde_json::from_str(testo).ok()?;
    let voci = v.as_object()?;
    if voci.is_empty() {
        return None;
    }
    let voce = voci.values().find(|x| x["email"].is_string());
    Some(match voce {
        Some(x) => Account {
            nome: x["email"].as_str().unwrap_or("collegato").to_string(),
            dettaglio: x["team_name"].as_str().map(str::to_string),
        },
        None => Account {
            nome: "collegato".into(),
            dettaglio: None,
        },
    })
}

/// Il file `auth.json` di grok: `GROK_HOME` o `~/.grok`.
pub fn file_grok() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("GROK_HOME").filter(|h| !h.is_empty()) {
        return Some(PathBuf::from(h).join("auth.json"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".grok/auth.json"))
}

/// L'account di grok dal suo file (assente = non collegato).
pub async fn account_grok() -> Result<Option<Account>, Errore> {
    let Some(f) = file_grok() else {
        return Ok(None);
    };
    match tokio::fs::read_to_string(&f).await {
        Ok(t) => Ok(leggi_grok(&t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Errore::interno(format!("{}: {e}", f.display()))),
    }
}

// ───────────────────────── accesso in corso ─────────────────────────

/// Quello che il comando di accesso ha stampato: l'url da aprire e il codice da scrivere.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Invito {
    pub url: Option<String>,
    pub codice_utente: Option<String>,
}

/// Toglie i colori ANSI.
fn pulisci(riga: &str) -> String {
    let mut out = String::with_capacity(riga.len());
    let mut it = riga.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            if it.peek() == Some(&'[') {
                it.next();
                for d in it.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Il primo `https://…` di una riga.
pub fn url_in(riga: &str) -> Option<String> {
    let i = riga.find("https://")?;
    let u: String = riga[i..]
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '"' && *c != '\'' && *c != '>')
        .collect();
    (u.len() > "https://".len()).then_some(u)
}

/// Un codice da scrivere: una riga sola come `ABCD-1234` (codex, grok).
pub fn codice_in(riga: &str) -> Option<String> {
    let t = riga.trim();
    let (a, b) = t.split_once('-')?;
    let ok = |s: &str| (4..=6).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric());
    (ok(a)
        && ok(b)
        && t.len() <= 13
        && t.chars()
            .any(|c| c.is_ascii_digit() || c.is_ascii_uppercase()))
    .then(|| t.to_string())
}

/// Un accesso partito: il processo resta vivo fino al ritorno (o a `scadenza`).
pub struct AccessoInCorso {
    figlio: Child,
    stdin: Option<ChildStdin>,
    pub invito: Invito,
}

impl std::fmt::Debug for AccessoInCorso {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccessoInCorso").finish_non_exhaustive()
    }
}

/// Lancia il comando di accesso e aspetta l'url (al massimo `attesa`).
pub async fn avvia(c: &ComandoAccesso, attesa: Duration) -> Result<AccessoInCorso, Errore> {
    let mut cmd = Command::new(&c.programma);
    cmd.args(&c.argomenti)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .kill_on_drop(true);
    for (k, v) in &c.ambiente {
        cmd.env(k, v);
    }
    let mut figlio = cmd
        .spawn()
        .map_err(|e| Errore::interno(format!("{} non parte: {e}", c.programma.display())))?;
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    if let Some(o) = figlio.stdout.take() {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut l = BufReader::new(o).lines();
            while let Ok(Some(r)) = l.next_line().await {
                let _ = tx.send(r);
            }
        });
    }
    if let Some(e) = figlio.stderr.take() {
        tokio::spawn(async move {
            let mut l = BufReader::new(e).lines();
            while let Ok(Some(r)) = l.next_line().await {
                let _ = tx.send(r);
            }
        });
    }
    let mut invito = Invito::default();
    let fine = tokio::time::Instant::now() + attesa;
    // Dopo l'url si aspetta ancora un poco il codice (arriva qualche riga dopo).
    let mut fine_codice: Option<tokio::time::Instant> = None;
    loop {
        let limite = fine_codice.unwrap_or(fine).min(fine);
        match tokio::time::timeout_at(limite, rx.recv()).await {
            Ok(Some(riga)) => {
                let r = pulisci(&riga);
                if invito.url.is_none()
                    && let Some(u) = url_in(&r)
                {
                    invito.url = Some(u);
                    fine_codice = Some(tokio::time::Instant::now() + Duration::from_millis(1500));
                } else if invito.url.is_some()
                    && invito.codice_utente.is_none()
                    && let Some(k) = codice_in(&r)
                {
                    invito.codice_utente = Some(k);
                    break;
                }
            }
            Ok(None) | Err(_) => break,
        }
    }
    if invito.url.is_none() {
        let _ = figlio.start_kill();
        return Err(Errore::interno(format!(
            "{}: nessun indirizzo di accesso stampato",
            c.programma.display()
        )));
    }
    let stdin = figlio.stdin.take();
    Ok(AccessoInCorso {
        figlio,
        stdin,
        invito,
    })
}

impl AccessoInCorso {
    /// Lo stdin del processo, per il codice incollato dall'utente (Claude Code).
    pub fn prendi_stdin(&mut self) -> Option<ChildStdin> {
        self.stdin.take()
    }

    /// Aspetta la fine: `riuscito`, `fallito` o `scaduto` (processo fermato).
    pub async fn attendi(mut self, scadenza: Duration) -> &'static str {
        match tokio::time::timeout(scadenza, self.figlio.wait()).await {
            Ok(Ok(s)) if s.success() => "riuscito",
            Ok(_) => "fallito",
            Err(_) => {
                let _ = self.figlio.start_kill();
                "scaduto"
            }
        }
    }
}

/// Scrive il codice incollato dall'utente sullo stdin del comando di accesso.
pub async fn scrivi_codice(s: &mut ChildStdin, codice: &str) -> Result<(), Errore> {
    s.write_all(format!("{}\n", codice.trim()).as_bytes())
        .await
        .map_err(|e| Errore::interno(format!("codice non scritto: {e}")))?;
    s.flush().await.map_err(|e| Errore::interno(e.to_string()))
}

#[cfg(test)]
#[path = "accesso_prove.rs"]
mod prove;
