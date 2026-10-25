//! Aggiornamenti per chi installa (CONTRATTO-GITHUB.md, sezione E).
//!
//! - `GET /aggiornamenti`: versione in opera (`versione.txt`), ultima Release di `XBOT_REPO` su
//!   GitHub, se ce n'è una più nuova. Al più una richiesta a GitHub ogni 6 ore (cache in memoria),
//!   `?forza=1` la rifà subito. Un errore di rete dà `disponibile: null` e `errore`, mai 500.
//! - `POST /aggiornamenti/avvia`: lancia staccato `XBOT_AGGIORNA_COMANDO` (predefinito
//!   `<XBOT_SORGENTE>/installa.sh --aggiorna --si`), uscita in `<dati>/aggiornamenti.log`.
//!   `XBOT_AGGIORNAMENTO=no` lo vieta; un solo aggiornamento alla volta.
//!
//! Variabili: `XBOT_REPO` (predefinito `AndyTrust/XBot`), `XBOT_AGGIORNAMENTO`
//! (`avvisa` | `auto` | `no`, predefinito `avvisa`), `XBOT_SORGENTE`, `XBOT_AGGIORNA_COMANDO`,
//! `XBOT_VERSIONE_FILE`, `XBOT_GITHUB_API` (predefinito `https://api.github.com`, per le prove).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use xbot_core::{Codice, Errore, ora_ms};

use crate::errore_http::ErroreHttp;
use crate::stato::Stato;

/// Quanto vale un controllo (6 ore).
pub const DURATA_CACHE_MS: i64 = 6 * 60 * 60 * 1000;
const REPO_PREDEFINITO: &str = "AndyTrust/XBot";
const API_PREDEFINITA: &str = "https://api.github.com";

/// Le impostazioni del controllo, dall'ambiente o date dalle prove.
#[derive(Debug, Clone)]
pub struct Impostazioni {
    pub repo: String,
    pub api: String,
    pub modo: String,
}

/// Le impostazioni imposte dalle prove al posto dell'ambiente (l'ambiente non si cambia senza
/// `unsafe`).
static PER_PROVE: Mutex<Option<Impostazioni>> = Mutex::new(None);

/// Per le prove: usa queste impostazioni invece dell'ambiente (`None` torna all'ambiente).
#[doc(hidden)]
pub fn imposta_per_prove(imp: Option<Impostazioni>) {
    if let Ok(mut g) = PER_PROVE.lock() {
        *g = imp;
    }
}

impl Impostazioni {
    /// Quelle delle prove, se ci sono, altrimenti quelle dell'ambiente.
    pub fn correnti() -> Self {
        PER_PROVE
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_else(Self::da_ambiente)
    }

    pub fn da_ambiente() -> Self {
        let leggi = |k: &str| {
            std::env::var(k)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let modo = match leggi("XBOT_AGGIORNAMENTO").as_deref() {
            Some(m @ ("avvisa" | "auto" | "no")) => m.to_string(),
            _ => "avvisa".into(),
        };
        Self {
            repo: leggi("XBOT_REPO").unwrap_or_else(|| REPO_PREDEFINITO.into()),
            api: leggi("XBOT_GITHUB_API")
                .unwrap_or_else(|| API_PREDEFINITA.into())
                .trim_end_matches('/')
                .to_string(),
            modo,
        }
    }
}

/// L'ultima Release letta da GitHub.
#[derive(Debug, Clone, PartialEq)]
pub struct Remota {
    pub tag: String,
    pub note_url: Option<String>,
}

/// Un controllo in cache: per quale repository, quando, con quale esito.
#[derive(Debug, Clone)]
struct Controllo {
    chiave: String,
    ms: i64,
    esito: Result<Remota, String>,
}

static CACHE: Mutex<Option<Controllo>> = Mutex::new(None);
static IN_CORSO: Mutex<Option<std::process::Child>> = Mutex::new(None);

/// I numeri di una versione `v2026.10.25.1` / `0.1.0`; `None` se non è fatta solo di numeri.
fn numeri(v: &str) -> Option<Vec<u64>> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    if v.is_empty() {
        return None;
    }
    v.split('.').map(|p| p.parse().ok()).collect()
}

/// La Release `remota` è più nuova di quella in opera? Con due versioni numeriche si confrontano
/// i numeri; altrimenti (un hash di commit) è nuova se è diversa.
pub fn piu_nuova(remota: &str, installata: &str) -> bool {
    let pulisci = |v: &str| v.trim().trim_start_matches(['v', 'V']).to_string();
    if pulisci(remota) == pulisci(installata) {
        return false;
    }
    match (numeri(remota), numeri(installata)) {
        (Some(r), Some(i)) => r > i,
        _ => true,
    }
}

/// Chiede a GitHub l'ultima Release.
pub async fn leggi_remota(imp: &Impostazioni) -> Result<Remota, String> {
    let url = format!("{}/repos/{}/releases/latest", imp.api, imp.repo);
    let cliente = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(format!("XBot/{}", xbot_core::VERSIONE))
        .build()
        .map_err(|e| format!("client HTTP: {e}"))?;
    let r = cliente
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("GitHub non risponde: {e}"))?;
    let stato = r.status();
    if !stato.is_success() {
        return Err(format!("GitHub risponde {stato}"));
    }
    let v: Value = r
        .json()
        .await
        .map_err(|e| format!("risposta di GitHub non valida: {e}"))?;
    let tag = v["tag_name"]
        .as_str()
        .filter(|t| !t.trim().is_empty())
        .ok_or("la Release non ha tag_name")?
        .to_string();
    Ok(Remota {
        tag,
        note_url: v["html_url"].as_str().map(str::to_string),
    })
}

/// La Release, dalla cache se ha meno di 6 ore (e `forza` è falso), altrimenti da GitHub.
/// Restituisce anche il momento del controllo.
pub async fn remota_con_cache(
    imp: &Impostazioni,
    forza: bool,
    ora: i64,
) -> (Result<Remota, String>, i64) {
    let chiave = format!("{}|{}", imp.api, imp.repo);
    if !forza
        && let Some(c) = CACHE.lock().ok().and_then(|g| g.clone())
        && c.chiave == chiave
        && ora - c.ms < DURATA_CACHE_MS
    {
        return (c.esito, c.ms);
    }
    let esito = leggi_remota(imp).await;
    if let Ok(mut g) = CACHE.lock() {
        *g = Some(Controllo {
            chiave,
            ms: ora,
            esito: esito.clone(),
        });
    }
    (esito, ora)
}

/// Il corpo di `GET /aggiornamenti`.
pub fn corpo(
    installata: &str,
    esito: &Result<Remota, String>,
    controllato_ms: i64,
    modo: &str,
) -> Value {
    match esito {
        Ok(r) => json!({
            "versione_installata": installata,
            "versione_remota": r.tag,
            "disponibile": piu_nuova(&r.tag, installata),
            "controllato_ms": controllato_ms,
            "modo": modo,
            "note_url": r.note_url,
        }),
        Err(e) => json!({
            "versione_installata": installata,
            "versione_remota": null,
            "disponibile": null,
            "controllato_ms": controllato_ms,
            "modo": modo,
            "note_url": null,
            "errore": e,
        }),
    }
}

#[derive(Deserialize)]
pub struct Forza {
    #[serde(default)]
    pub forza: Option<String>,
}

/// `GET /aggiornamenti[?forza=1]`.
pub async fn stato(State(s): State<Stato>, Query(f): Query<Forza>) -> Json<Value> {
    let imp = Impostazioni::correnti();
    let forza = matches!(f.forza.as_deref(), Some("1" | "true" | "si"));
    let installata = xbot_core::versione_installata(&s.servizi.cartella_dati);
    let (esito, ms) = remota_con_cache(&imp, forza, ora_ms()).await;
    Json(corpo(&installata, &esito, ms, &imp.modo))
}

/// Il comando dell'aggiornamento: `XBOT_AGGIORNA_COMANDO` (lanciato con `sh -c`), altrimenti
/// `<XBOT_SORGENTE>/installa.sh --aggiorna --si`; `None` se non si sa dove sta il sorgente.
pub fn comando() -> Option<(String, Vec<String>)> {
    let leggi = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    if let Some(c) = leggi("XBOT_AGGIORNA_COMANDO") {
        return Some(("sh".into(), vec!["-c".into(), c]));
    }
    let sorgente = PathBuf::from(leggi("XBOT_SORGENTE")?);
    Some((
        sorgente.join("installa.sh").to_string_lossy().into_owned(),
        vec!["--aggiorna".into(), "--si".into()],
    ))
}

/// Lancia l'aggiornamento staccato dal server (nuovo gruppo di processi), con l'uscita in
/// `<dati>/aggiornamenti.log`. Errore se il modo è `no`, se manca il comando o se uno è in corso.
pub fn avvia_in(
    dati: &Path,
    modo: &str,
    cmd: Option<(String, Vec<String>)>,
) -> Result<u32, Errore> {
    if modo == "no" {
        return Err(Errore::nuovo(
            Codice::AggiornamentiDisattivati,
            "gli aggiornamenti dall'app sono spenti (XBOT_AGGIORNAMENTO=no)",
        ));
    }
    let Some((prog, argomenti)) = cmd else {
        return Err(Errore::nuovo(
            Codice::SorgenteSconosciuta,
            "non so dove sta il sorgente: serve XBOT_SORGENTE (o XBOT_AGGIORNA_COMANDO)",
        ));
    };
    let mut g = IN_CORSO
        .lock()
        .map_err(|_| Errore::interno("stato dell'aggiornamento non leggibile"))?;
    if let Some(figlio) = g.as_mut()
        && matches!(figlio.try_wait(), Ok(None))
    {
        return Err(Errore::nuovo(
            Codice::InCorso,
            "un aggiornamento è già in corso",
        ));
    }
    std::fs::create_dir_all(dati)
        .map_err(|e| Errore::interno(format!("{}: {e}", dati.display())))?;
    let log = dati.join("aggiornamenti.log");
    let apri = || {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .map_err(|e| Errore::interno(format!("{}: {e}", log.display())))
    };
    let (out, err) = (apri()?, apri()?);
    {
        use std::io::Write;
        let mut o = &out;
        let _ = writeln!(
            o,
            "--- {} aggiornamento avviato dall'app",
            chrono::Local::now().format("%Y-%m-%d %H:%M")
        );
    }
    let mut c = std::process::Command::new(&prog);
    c.args(&argomenti)
        .stdin(std::process::Stdio::null())
        .stdout(out)
        .stderr(err);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
    }
    let figlio = c
        .spawn()
        .map_err(|e| Errore::interno(format!("{prog} non parte: {e}")))?;
    let pid = figlio.id();
    *g = Some(figlio);
    tracing::info!(pid, "aggiornamento avviato");
    Ok(pid)
}

/// `POST /aggiornamenti/avvia` → 202 `{avviato: true}`.
pub async fn avvia(State(s): State<Stato>) -> Result<(StatusCode, Json<Value>), ErroreHttp> {
    let imp = Impostazioni::correnti();
    let dati = s.servizi.cartella_dati.clone();
    let pid = tokio::task::spawn_blocking(move || avvia_in(&dati, &imp.modo, comando()))
        .await
        .map_err(|e| Errore::interno(e.to_string()))??;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "avviato": true, "pid": pid })),
    ))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn confronto_dei_tag() {
        assert!(piu_nuova("v2026.10.26", "v2026.10.25"));
        assert!(piu_nuova("v2026.10.25.1", "v2026.10.25"));
        assert!(!piu_nuova("v2026.10.25", "2026.10.25"));
        assert!(!piu_nuova("v2026.10.24", "v2026.10.25"));
        assert!(piu_nuova("v2026.10.25", "0.1.0"));
        assert!(piu_nuova("v2026.10.25", "abc1234"));
        assert!(!piu_nuova("abc1234", "abc1234"));
    }

    #[test]
    fn corpo_con_errore() {
        let v = corpo("0.1.0", &Err("GitHub non risponde".into()), 5, "avvisa");
        assert_eq!(v["disponibile"], Value::Null);
        assert_eq!(v["errore"], "GitHub non risponde");
        assert_eq!(v["versione_installata"], "0.1.0");
        let v = corpo(
            "v2026.10.25",
            &Ok(Remota {
                tag: "v2026.10.26".into(),
                note_url: Some("https://x/r".into()),
            }),
            5,
            "auto",
        );
        assert_eq!(v["disponibile"], true);
        assert_eq!(v["note_url"], "https://x/r");
        assert!(v.get("errore").is_none());
    }

    #[test]
    fn modo_no_e_sorgente() {
        let dir = std::env::temp_dir().join(format!("xbot-agg-{}", xbot_core::nuovo_id()));
        let e = avvia_in(&dir, "no", Some(("true".into(), vec![]))).unwrap_err();
        assert_eq!(e.codice, Codice::AggiornamentiDisattivati);
        let e = avvia_in(&dir, "avvisa", None).unwrap_err();
        assert_eq!(e.codice, Codice::SorgenteSconosciuta);
    }

    #[test]
    fn un_avvio_alla_volta() {
        let dir = std::env::temp_dir().join(format!("xbot-agg-{}", xbot_core::nuovo_id()));
        let lento = Some(("sh".into(), vec!["-c".into(), "sleep 2; echo fatto".into()]));
        avvia_in(&dir, "avvisa", lento.clone()).unwrap();
        let e = avvia_in(&dir, "auto", lento).unwrap_err();
        assert_eq!(e.codice, Codice::InCorso);
        std::thread::sleep(Duration::from_millis(2300));
        avvia_in(&dir, "avvisa", Some(("true".into(), vec![]))).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let log = std::fs::read_to_string(dir.join("aggiornamenti.log")).unwrap();
        assert!(
            log.contains("fatto") && log.matches("aggiornamento avviato").count() == 2,
            "{log}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
