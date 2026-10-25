//! Gli strumenti condivisi fra il server interno (assistenti) e il canale dei client MCP esterni:
//! cassaforte, quaderno, memoria, assistenti. Ognuno restituisce un testo per l'assistente.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use rmcp::model::CallToolResponse;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;
use xbot_core::{Risposta, TipoRichiesta, ora_ms};
use xbot_db::{assistenti, cassaforte, cassaforte_voci, impostazioni};

use super::{TurnoMcp, testo};
use crate::Stato;

/// L'esito di uno strumento: testo per l'assistente, o errore leggibile.
pub type Esito = Result<String, String>;

pub fn esito(e: Esito) -> CallToolResponse {
    match e {
        Ok(t) => testo(true, t),
        Err(t) => testo(false, t),
    }
}

fn da<T>(r: Risposta<T>) -> Result<T, String> {
    r.map_err(|e| e.testo)
}

/// La cartella degli script esterni (`quaderno.py`, `cerca_memoria.py`): `XBOT_STRUMENTI_DIR`,
/// altrimenti `<dati>/strumenti`.
fn cartella_strumenti(m: &Stato) -> PathBuf {
    std::env::var_os("XBOT_STRUMENTI_DIR")
        .filter(|d| !d.is_empty())
        .map_or_else(|| m.servizi.cartella_dati.join("strumenti"), PathBuf::from)
}

/// Lo script esterno, se c'è su questa macchina (`<cartella degli strumenti>/<nome>`).
fn script_esterno(m: &Stato, nome: &str) -> Option<PathBuf> {
    let p = cartella_strumenti(m).join(nome);
    p.is_file().then_some(p)
}

/// Lancia un programma senza shell, con un tempo massimo; restituisce stdout (al più 20 kB).
pub async fn programma(
    prog: &str,
    argomenti: &[String],
    ingresso: Option<&str>,
    tempo: Duration,
) -> Esito {
    let mut cmd = tokio::process::Command::new(prog);
    cmd.args(argomenti)
        .stdin(if ingresso.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut figlio = cmd.spawn().map_err(|e| format!("{prog} non parte: {e}"))?;
    if let (Some(t), Some(mut s)) = (ingresso, figlio.stdin.take()) {
        let _ = s.write_all(t.as_bytes()).await;
    }
    let uscita = tokio::time::timeout(tempo, figlio.wait_with_output())
        .await
        .map_err(|_| format!("{prog}: tempo scaduto"))?
        .map_err(|e| format!("{prog}: {e}"))?;
    let mut out = String::from_utf8_lossy(&uscita.stdout).into_owned();
    if out.len() > 20_000 {
        let mut fine = 20_000;
        while !out.is_char_boundary(fine) {
            fine -= 1;
        }
        out.truncate(fine);
        out.push_str("\n…");
    }
    if uscita.status.success() {
        Ok(out)
    } else {
        let err = String::from_utf8_lossy(&uscita.stderr);
        Err(format!("{prog}: {} {}", uscita.status, err.trim()))
    }
}

/// `cassaforte_elenco`: id, genere, titolo e siti. Mai i valori.
pub async fn cassaforte_elenco(m: &Stato, _t: Option<&TurnoMcp>) -> Esito {
    let voci = da(m.db.esegui(|c| cassaforte::elenca(c, None)).await)?;
    let v: Vec<Value> = voci
        .iter()
        .map(|x| json!({ "id": x.id, "genere": x.genere, "titolo": x.titolo, "siti": x.siti }))
        .collect();
    Ok(Value::Array(v).to_string())
}

/// Il valore principale di una voce: `valore`, `password`, `numero`, altrimenti tutti i campi.
fn valore_principale(genere: &str, campi: &Value) -> String {
    let k = match genere {
        "segreto" => "valore",
        "accesso" => "password",
        "carta" => "numero",
        _ => "",
    };
    campi
        .get(k)
        .and_then(Value::as_str)
        .map_or_else(|| campi.to_string(), str::to_string)
}

fn scrivi_privato(file: &Path, testo: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(d) = file.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(file)?.write_all(testo.as_bytes())
}

/// `cassaforte_chiedi`: richiesta all'utente; se approva, il valore va in un file 0600 della
/// cartella del turno (cancellata a fine turno), mai nel testo della chat.
pub async fn cassaforte_chiedi(m: &Stato, t: &TurnoMcp, id: &str, motivo: &str) -> Esito {
    let id2 = id.to_string();
    let voce = da(m
        .db
        .esegui(move |c| cassaforte::leggi_riferimento(c, &id2))
        .await)?;
    let variabile = cassaforte_voci::nome_variabile(&voce.titolo, &voce.riferimento);
    let carico = json!({
        "voce_id": voce.id, "titolo": voce.titolo, "genere": voce.genere,
        "motivo": motivo, "variabile": variabile, "turno_id": t.turno_id,
    });
    let attesa = m.motore.s2().attesa();
    let (stato, ris) =
        da(super::permesso::attendi(m, t, TipoRichiesta::Cassaforte, carico, attesa).await)?;
    match stato.as_str() {
        "approvata" => {}
        "negata" => return Err("L'utente ha negato l'uso di questa voce.".into()),
        "ritirata" => return Err("Richiesta ritirata: il turno è stato interrotto.".into()),
        _ => return Err("L'utente non ha risposto in tempo.".into()),
    }
    let rid = ris["richiesta_id"].as_str().map(str::to_string);
    let chiave = m.servizi.chiave().map_err(|e| e.testo)?;
    let (vid, sid, aid) = (
        voce.id.clone(),
        t.sessione_id.clone(),
        t.assistente_id.clone(),
    );
    let campi = da(m
        .db
        .esegui(move |c| {
            let testo = cassaforte::leggi_campi(c, &vid, &chiave)?;
            cassaforte_voci::registra_uso(
                c,
                &vid,
                Some(&sid),
                Some(&aid),
                "variabile_ambiente",
                rid.as_deref(),
            )?;
            Ok(serde_json::from_str::<Value>(testo.esponi()).unwrap_or(Value::Null))
        })
        .await)?;
    let valore = xbot_core::SegretoTesto::nuovo(valore_principale(&voce.genere, &campi));
    let file = t.cartella.join("cassaforte").join(&variabile);
    scrivi_privato(&file, valore.esponi()).map_err(|e| format!("file del valore: {e}"))?;
    Ok(format!(
        "Approvato. Il valore è nel file {f} (variabile {variabile}). Usalo solo in Bash, per esempio \
         `export {variabile}=\"$(cat '{f}')\"`, e non scriverlo mai in chat. Il file sparisce a fine turno.",
        f = file.display()
    ))
}

fn nome_quaderno(nome: &str) -> String {
    nome.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

/// `quaderno_scrivi`: con `<strumenti>/quaderno.py` se c'è, altrimenti una riga datata
/// nel file del quaderno (`quaderno_percorso` dell'assistente o `<dati>/quaderni/<agente>.md`).
pub async fn quaderno_scrivi(m: &Stato, t: Option<&TurnoMcp>, agente: &str, testo: &str) -> Esito {
    if testo.trim().is_empty() {
        return Err("il testo è vuoto".into());
    }
    let a = match t {
        Some(t) => {
            let aid = t.assistente_id.clone();
            Some(da(m.db.esegui(move |c| assistenti::leggi(c, &aid)).await)?)
        }
        None => None,
    };
    let agente = if agente.trim().is_empty() {
        a.as_ref()
            .and_then(|a| a["nome"].as_str())
            .map(nome_quaderno)
            .ok_or("manca il nome del quaderno")?
    } else {
        nome_quaderno(agente)
    };
    if agente.is_empty() || agente.contains('/') || agente.contains("..") {
        return Err("nome del quaderno non valido".into());
    }
    if let Some(s) = script_esterno(m, "quaderno.py") {
        let marcato = [
            "DA SALVARE:",
            "ERRORE DA SALVARE:",
            "DA VERIFICARE:",
            "FONTE:",
            "PROPOSTA:",
        ]
        .iter()
        .any(|k| testo.contains(k));
        let mut arg = vec![s.to_string_lossy().into_owned()];
        let r = if marcato {
            arg.extend(["raccogli".into(), agente.clone()]);
            programma("python3", &arg, Some(testo), Duration::from_secs(30)).await
        } else {
            arg.extend([
                "scrivi".into(),
                agente.clone(),
                "--imparato".into(),
                testo.into(),
            ]);
            programma("python3", &arg, None, Duration::from_secs(30)).await
        };
        match r {
            Ok(out) => {
                registra_scrittura(m, t, "quaderno", &format!("quaderno.py:{agente}")).await;
                return Ok(format!("Salvato nel quaderno di {agente}. {}", out.trim()));
            }
            Err(e) => tracing::info!(errore = %e, "quaderno.py non ha scritto: ripiego sul file"),
        }
    }
    let file = a
        .as_ref()
        .and_then(|a| a["quaderno_percorso"].as_str())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            m.servizi
                .cartella_dati
                .join("quaderni")
                .join(format!("{agente}.md"))
        });
    let ora = chrono::Local::now().format("%Y-%m-%d %H:%M");
    let riga = format!("- {ora} · {}\n", testo.trim().replace('\n', " "));
    let f2 = file.clone();
    tokio::task::spawn_blocking(move || {
        use std::io::Write;
        if let Some(d) = f2.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&f2)?
            .write_all(riga.as_bytes())
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("quaderno {}: {e}", file.display()))?;
    registra_scrittura(m, t, "quaderno", &file.to_string_lossy()).await;
    Ok(format!("Salvato in {}.", file.display()))
}

async fn registra_scrittura(m: &Stato, t: Option<&TurnoMcp>, tipo: &'static str, percorso: &str) {
    let (sid, aid) = (
        t.map(|t| t.sessione_id.clone()),
        t.map(|t| t.assistente_id.clone()),
    );
    let p = percorso.to_string();
    let r =
        m.db.esegui(move |c| {
            c.execute(
                "INSERT INTO memoria_scrittura (sessione_id, assistente_id, tipo, percorso, ms)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                (sid, aid, tipo, p, ora_ms()),
            )
            .map_err(|e| xbot_core::Errore::database(e.to_string()))?;
            Ok(())
        })
        .await;
    if let Err(e) = r {
        tracing::warn!(errore = %e, "memoria_scrittura non registrata");
    }
}

/// La radice della memoria: la cartella dell'assistente, `memoria.cartella`, o la cartella di
/// `memoria.ingresso`.
async fn radice_memoria(
    m: &Stato,
    assistente_id: Option<&str>,
) -> Option<(PathBuf, Option<PathBuf>)> {
    let aid = assistente_id.map(str::to_string);
    m.db.esegui(move |c| {
        let ingresso =
            impostazioni::leggi(c, "memoria.ingresso")?.and_then(|v| v.as_str().map(PathBuf::from));
        if let Some(a) = aid
            && let Some(p) = assistenti::leggi(c, &a)?["memoria_cartella"].as_str()
        {
            return Ok(Some((PathBuf::from(p), ingresso)));
        }
        if let Some(p) =
            impostazioni::leggi(c, "memoria.cartella")?.and_then(|v| v.as_str().map(PathBuf::from))
        {
            return Ok(Some((p, ingresso)));
        }
        Ok(ingresso
            .clone()
            .and_then(|i| i.parent().map(|p| (p.to_path_buf(), Some(i.clone())))))
    })
    .await
    .ok()
    .flatten()
}

/// `memoria_cerca` / `cerca_memoria`: `cerca_memoria.py` se c'è, altrimenti un grep nei `.md`.
pub async fn memoria_cerca(m: &Stato, assistente_id: Option<&str>, parole: &str) -> Esito {
    let parole: Vec<String> = parole
        .split_whitespace()
        .map(str::to_string)
        .take(12)
        .collect();
    if parole.is_empty() {
        return Err("servono delle parole".into());
    }
    if let Some(s) = script_esterno(m, "cerca_memoria.py") {
        let mut arg = vec![s.to_string_lossy().into_owned()];
        arg.extend(parole.iter().cloned());
        if let Ok(out) = programma("python3", &arg, None, Duration::from_secs(60)).await {
            return Ok(out);
        }
    }
    let (radice, _) = radice_memoria(m, assistente_id)
        .await
        .ok_or("nessuna cartella di memoria impostata")?;
    let minuscole: Vec<String> = parole.iter().map(|p| p.to_lowercase()).collect();
    let trovati = tokio::task::spawn_blocking(move || cerca_nei_file(&radice, &minuscole, 30))
        .await
        .map_err(|e| e.to_string())?;
    if trovati.is_empty() {
        Ok("Nessun risultato.".into())
    } else {
        Ok(trovati.join("\n"))
    }
}

fn cerca_nei_file(radice: &Path, parole: &[String], massimo: usize) -> Vec<String> {
    let mut fuori = Vec::new();
    let mut da_vedere = vec![radice.to_path_buf()];
    let mut visti = 0;
    while let Some(d) = da_vedere.pop() {
        let Ok(voci) = std::fs::read_dir(&d) else {
            continue;
        };
        for v in voci.flatten() {
            let p = v.path();
            let nome = v.file_name().to_string_lossy().into_owned();
            if nome.starts_with('.') {
                continue;
            }
            if p.is_dir() {
                da_vedere.push(p);
            } else if nome.ends_with(".md") {
                visti += 1;
                if visti > 5000 {
                    return fuori;
                }
                let Ok(t) = std::fs::read_to_string(&p) else {
                    continue;
                };
                for (n, riga) in t.lines().enumerate() {
                    let r = riga.to_lowercase();
                    if parole.iter().all(|w| r.contains(w)) {
                        let rel = p.strip_prefix(radice).unwrap_or(&p).display().to_string();
                        fuori.push(format!(
                            "{rel}:{}: {}",
                            n + 1,
                            riga.trim().chars().take(240).collect::<String>()
                        ));
                        if fuori.len() >= massimo {
                            return fuori;
                        }
                    }
                }
            }
        }
    }
    fuori
}

/// `leggi_memoria {percorso?}`: la porta d'ingresso o un file dentro la radice della memoria.
pub async fn leggi_memoria(m: &Stato, assistente_id: Option<&str>, percorso: &str) -> Esito {
    let (radice, ingresso) = radice_memoria(m, assistente_id)
        .await
        .ok_or("nessuna cartella di memoria impostata")?;
    let file = if percorso.trim().is_empty() {
        ingresso.unwrap_or_else(|| radice.join("Memoria.md"))
    } else {
        radice.join(percorso.trim_start_matches('/'))
    };
    let (rc, fc) = (std::fs::canonicalize(&radice), std::fs::canonicalize(&file));
    match (rc, fc) {
        (Ok(r), Ok(f)) if f.starts_with(&r) || percorso.trim().is_empty() => {
            let t = tokio::fs::read_to_string(&f)
                .await
                .map_err(|e| format!("{}: {e}", f.display()))?;
            Ok(t.chars().take(200_000).collect())
        }
        (_, Err(_)) => Err(format!("file non trovato: {percorso}")),
        _ => Err("il percorso esce dalla memoria".into()),
    }
}

/// `assistenti_elenco` / `elenco_assistenti`: menzione, nome, titolo, descrizione.
pub async fn assistenti_elenco(m: &Stato) -> Result<Value, String> {
    let v = da(m.db.esegui(|c| assistenti::elenca(c, false)).await)?;
    // `menzione` la aggiunge `assistenti::elenca` (S1): unica fonte delle menzioni.
    Ok(Value::Array(
        v.iter()
            .map(|a| json!({ "menzione": format!("@{}", a["menzione"].as_str().unwrap_or_default()), "nome": a["nome"], "titolo": a["titolo"], "descrizione": a["descrizione"] }))
            .collect(),
    ))
}
