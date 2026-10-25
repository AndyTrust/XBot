//! Lo specchio con il registro dei lavori (`lavori.py`, nella cartella di `XBOT_REGISTRO_DIR`).
//! XBot è la fonte; per un periodo scrive le sue voci anche nel registro (formato di `lavori.py`,
//! file `attivi/xbot__<sessione>__<agente>__<progetto>.json`) e importa nella lavagna le voci che
//! gli altri costruttori scrivono con `lavori.py`. Si accende con `XBOT_REGISTRO_DIR`.
//! A ogni giro (ogni 60 secondi e dopo ogni scrittura sulla lavagna) il risultato è lo stesso:
//! il giro è idempotente.

use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Datelike, NaiveDate, Timelike, Utc};
use serde_json::{Value, json};
use xbot_db::Db;
use xbot_db::lavagna::{self, MORTA_MS, VoceRegistro};

use crate::Stato;

pub const VARIABILE: &str = "XBOT_REGISTRO_DIR";
const PREFISSO: &str = "xbot__";
const GIRO: Duration = Duration::from_secs(60);

/// La cartella del registro, se lo specchio è acceso.
pub fn cartella() -> Option<PathBuf> {
    std::env::var(VARIABILE)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
}

/// Come `slug` di `lavori.py`: minuscole, tutto ciò che non è lettera o cifra diventa `-`.
pub fn slug(t: &str) -> String {
    let mut s = String::new();
    for c in t.to_lowercase().replace('\'', " ").chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') {
            s.push('-');
        }
    }
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "senza-nome".into() } else { s }
}

/// L'ultima domenica del mese (di 31 giorni) alle 01:00 UTC, quando cambia l'ora.
fn cambio_ora(anno: i32, mese: u32) -> Option<chrono::NaiveDateTime> {
    let ultimo = NaiveDate::from_ymd_opt(anno, mese, 31)?;
    let domenica =
        ultimo - chrono::Duration::days(i64::from(ultimo.weekday().num_days_from_sunday()));
    domenica.and_hms_opt(1, 0, 0)
}

/// L'ora italiana leggibile (`AAAA-MM-GG HH:MM`) da un tempo assoluto: ora legale dall'ultima
/// domenica di marzo all'ultima di ottobre, alle 01:00 UTC.
pub fn ora_roma(secondi: i64) -> String {
    let u: DateTime<Utc> = DateTime::from_timestamp(secondi, 0).unwrap_or_default();
    let legale = match (cambio_ora(u.year(), 3), cambio_ora(u.year(), 10)) {
        (Some(inizio), Some(fine)) => u.naive_utc() >= inizio && u.naive_utc() < fine,
        _ => false,
    };
    let l = u + chrono::Duration::hours(if legale { 2 } else { 1 });
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        l.year(),
        l.month(),
        l.day(),
        l.hour(),
        l.minute()
    )
}

fn nome_file(v: &Value) -> String {
    let sess: String = slug(v["sessione_id"].as_str().unwrap_or("x"))
        .chars()
        .take(24)
        .collect();
    let ag: String = slug(v["agente"].as_str().unwrap_or("xbot"))
        .chars()
        .take(20)
        .collect();
    format!(
        "{PREFISSO}{sess}__{ag}__{}.json",
        slug(v["progetto"].as_str().unwrap_or(""))
    )
}

/// Il contenuto di una presa nel formato di `lavori.py`.
pub fn presa(v: &Value) -> Value {
    let (inizio, battito) = (
        v["inizio_ms"].as_i64().unwrap_or(0) / 1000,
        v["battito_ms"].as_i64().unwrap_or(0) / 1000,
    );
    json!({
        "progetto": v["progetto"], "cosa": v["cosa"], "agente": v["agente"],
        "macchina": "xbot", "sessione": v["sessione_id"],
        "file": v["file_json"], "risorse": v["risorse_json"], "max_min": 120, "pid": null,
        "inizio": ora_roma(inizio), "battito": ora_roma(battito),
        "ts": inizio, "ts_battito": battito, "origine": "xbot",
    })
}

fn scrivi_atomico(f: &Path, d: &Value) -> std::io::Result<()> {
    let tmp = f.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(d).unwrap_or_default())?;
    std::fs::rename(tmp, f)
}

/// Una presa del registro come voce da importare; `None` se chiusa, morta o non nostra.
fn leggi_presa(f: &Path, ora_s: i64) -> Option<VoceRegistro> {
    let nome = f.file_name()?.to_str()?.to_string();
    if nome.starts_with('.') || nome.starts_with(PREFISSO) || !nome.ends_with(".json") {
        return None;
    }
    let testo = std::fs::read_to_string(f).ok()?;
    let d: Value = serde_json::from_str(testo.trim_start_matches('\u{feff}')).ok()?;
    if d["chiuso"].as_i64().unwrap_or(0) > 0 || d["chiuso"].as_f64().unwrap_or(0.0) > 0.0 {
        return None;
    }
    let mtime_s = std::fs::metadata(f)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    let battito = d["ts_battito"].as_i64().unwrap_or(mtime_s);
    if (ora_s - battito) * 1000 > MORTA_MS {
        return None;
    }
    let lista = |k: &str| -> Vec<String> {
        d[k].as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    Some(VoceRegistro {
        chiave: nome,
        agente: d["agente"].as_str().unwrap_or("?").to_string(),
        progetto: d["progetto"].as_str().unwrap_or("").to_string(),
        cosa: d["cosa"]
            .as_str()
            .unwrap_or("(senza descrizione)")
            .to_string(),
        file: lista("file"),
        risorse: lista("risorse"),
        inizio_ms: d["ts"].as_i64().unwrap_or(battito) * 1000,
        battito_ms: battito * 1000,
    })
}

/// Un giro: scrive le voci vive di XBot, toglie i file di chi ha finito, importa quelle degli altri.
/// Restituisce (scritte, importate).
pub async fn giro(db: &Db, dir: &Path) -> std::io::Result<(usize, usize)> {
    let attivi = dir.join("attivi");
    std::fs::create_dir_all(&attivi)?;
    let vive = db
        .esegui(lavagna::voci_xbot_vive)
        .await
        .map_err(|e| std::io::Error::other(e.testo))?;
    let mut nomi = Vec::new();
    for v in &vive {
        let n = nome_file(v);
        scrivi_atomico(&attivi.join(&n), &presa(v))?;
        nomi.push(n);
    }
    let mut importate = Vec::new();
    let ora_s = xbot_core::ora_ms() / 1000;
    for e in std::fs::read_dir(&attivi)?.flatten() {
        let nome = e.file_name().to_string_lossy().to_string();
        if nome.starts_with(PREFISSO) {
            if !nomi.contains(&nome) {
                let _ = std::fs::remove_file(e.path());
            }
        } else if let Some(v) = leggi_presa(&e.path(), ora_s) {
            importate.push(v);
        }
    }
    let presenti: Vec<String> = importate.iter().map(|v| v.chiave.clone()).collect();
    let n = importate.len();
    db.esegui(move |c| {
        for v in &importate {
            lavagna::importa_registro(c, v)?;
        }
        lavagna::chiudi_registro_assenti(c, &presenti)?;
        Ok(())
    })
    .await
    .map_err(|e| std::io::Error::other(e.testo))?;
    Ok((vive.len(), n))
}

/// Un giro subito, se lo specchio è acceso (dopo ogni scrittura sulla lavagna).
pub async fn subito(db: &Db) {
    if let Some(d) = cartella()
        && let Err(e) = giro(db, &d).await
    {
        tracing::warn!(errore = %e, "specchio del registro: giro non riuscito");
    }
}

/// Il ciclo di fondo (spento se manca `XBOT_REGISTRO_DIR`).
pub async fn ciclo(stato: Stato) {
    let Some(dir) = cartella() else { return };
    tracing::info!(cartella = %dir.display(), "specchio del registro dei lavori acceso");
    loop {
        if let Err(e) = giro(&stato.db, &dir).await {
            tracing::warn!(errore = %e, "specchio del registro: giro non riuscito");
        }
        tokio::time::sleep(GIRO).await;
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn ora_italiana_con_ora_legale() {
        // 2026-10-25 11:00 UTC: l'ora legale è finita alle 01:00 UTC, quindi +1.
        assert_eq!(ora_roma(1_792_926_000), "2026-10-25 12:00");
        // 2026-07-01 10:00 UTC → 12:00.
        assert_eq!(ora_roma(1_782_900_000), "2026-07-01 12:00");
    }

    #[tokio::test]
    async fn giro_scrive_importa_e_chiude() {
        use xbot_db::lavagna::NuovaVoce;
        let db = Db::apri(":memory:").unwrap();
        let dir = std::env::temp_dir().join(format!("xbot-specchio-{}", xbot_core::nuovo_id()));
        let (a, s) = ("a1".to_string(), "s1".to_string());
        db.esegui(move |c| {
            c.execute("INSERT INTO assistente (id, nome, creato_ms, aggiornato_ms) VALUES ('a1','A',0,0)", [])
                .unwrap();
            c.execute("INSERT INTO sessione (id, assistente_id, creata_ms, aggiornata_ms) VALUES ('s1','a1',0,0)", [])
                .unwrap();
            lavagna::scrivi(c, &NuovaVoce {
                assistente_id: &a, sessione_id: &s, agente: "alfa", macchina_id: None,
                progetto: "XBot", cosa: "prova", file: vec!["web".into()], risorse: vec![],
            })?;
            Ok(())
        })
        .await
        .unwrap();
        // una presa di lavori.py, viva
        std::fs::create_dir_all(dir.join("attivi")).unwrap();
        let ora = xbot_core::ora_ms() / 1000;
        std::fs::write(
            dir.join("attivi/mario__123__costruttore__prova.json"),
            json!({"progetto": "Prova", "cosa": "rifaccio web", "agente": "costruttore", "macchina": "mario",
                   "file": ["web/app.js"], "risorse": [], "ts": ora, "ts_battito": ora}).to_string(),
        )
        .unwrap();
        let (scritte, importate) = giro(&db, &dir).await.unwrap();
        assert_eq!((scritte, importate), (1, 1));
        let nomi: Vec<String> = std::fs::read_dir(dir.join("attivi"))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(
            nomi.iter().any(|n| n == "xbot__s1__alfa__xbot.json"),
            "{nomi:?}"
        );
        let f: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("attivi/xbot__s1__alfa__xbot.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(f["macchina"], "xbot");
        assert_eq!(f["file"], json!(["web"]));
        let vive = db.esegui(|c| lavagna::attive(c, None, None)).await.unwrap();
        assert_eq!(vive.len(), 2);
        assert!(
            vive.iter()
                .any(|v| v["origine"] == "registro" && v["agente"] == "costruttore")
        );
        // la presa degli altri sparisce: la voce si chiude; XBot chiude la sua: il file sparisce
        std::fs::remove_file(dir.join("attivi/mario__123__costruttore__prova.json")).unwrap();
        db.esegui(|c| lavagna::chiudi(c, "a1", "s1", "fatto").map(|_| ()))
            .await
            .unwrap();
        let (scritte, importate) = giro(&db, &dir).await.unwrap();
        assert_eq!((scritte, importate), (0, 0));
        assert!(!dir.join("attivi/xbot__s1__alfa__xbot.json").exists());
        assert!(
            db.esegui(|c| lavagna::attive(c, None, None))
                .await
                .unwrap()
                .is_empty()
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn slug_come_lavori_py() {
        assert_eq!(slug("Alfa Chat/Master"), "alfa-chat-master");
        assert_eq!(slug(""), "senza-nome");
        assert_eq!(slug("l'agente"), "l-agente");
    }
}
