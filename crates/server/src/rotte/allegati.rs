//! `POST /sessioni/{id}/allegati` (multipart → blob), `POST /sessioni/{id}/sposta`, e
//! l'aiuto comune per leggere un file da un modulo multipart.

use axum::extract::multipart::{Multipart, MultipartRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use xbot_core::{Codice, Errore, Risposta};
use xbot_db::allegati;

use super::json_db;
use crate::bus::Segnale;
use crate::errore_http::{Corpo, ErroreHttp, RisHttp};
use crate::stato::Stato;

/// Un file arrivato in un modulo multipart.
pub(crate) struct FileCaricato {
    pub nome: String,
    pub tipo: Option<String>,
    pub byte: Vec<u8>,
}

fn rifiuto(e: MultipartRejection) -> ErroreHttp {
    ErroreHttp(Errore::non_valida(format!(
        "modulo multipart non valido: {}",
        e.body_text()
    )))
}

/// Legge il primo campo che si chiama `campo` (al massimo `max` byte).
pub(crate) async fn leggi_file(
    mp: Result<Multipart, MultipartRejection>,
    campo: &str,
    max: usize,
) -> Result<FileCaricato, ErroreHttp> {
    let mut mp = mp.map_err(rifiuto)?;
    let troppo = || {
        Errore::nuovo(
            Codice::CorpoTroppoGrande,
            format!("il file supera {} MB", max / (1024 * 1024)),
        )
    };
    loop {
        let f = mp.next_field().await.map_err(|e| {
            if e.status().as_u16() == 413 {
                troppo()
            } else {
                Errore::non_valida(e.body_text())
            }
        })?;
        let Some(f) = f else {
            return Err(Errore::non_valida(format!("manca il campo «{campo}»")).into());
        };
        if f.name() != Some(campo) {
            continue;
        }
        let nome = f.file_name().unwrap_or("file").to_string();
        let tipo = f.content_type().map(str::to_string);
        let byte = f.bytes().await.map_err(|e| {
            if e.status().as_u16() == 413 {
                troppo()
            } else {
                Errore::non_valida(e.body_text())
            }
        })?;
        if byte.len() > max {
            return Err(troppo().into());
        }
        if byte.is_empty() {
            return Err(Errore::non_valida("il file è vuoto").into());
        }
        return Ok(FileCaricato {
            nome,
            tipo,
            byte: byte.to_vec(),
        });
    }
}

pub const MAX_ALLEGATO: usize = 25 * 1024 * 1024;

/// Salva l'allegato: blob, riga `allegato` e una copia con il nome originale accanto al blob
/// (`<hash>.d/<nome>`), così il provider riconosce il tipo dall'estensione.
pub async fn carica(
    State(s): State<Stato>,
    Path(sessione): Path<String>,
    mp: Result<Multipart, MultipartRejection>,
) -> RisHttp<Value> {
    let f = leggi_file(mp, "file", MAX_ALLEGATO).await?;
    let blob = s.servizi.blob.clone();
    json_db(&s, move |c| {
        xbot_db::sessioni::leggi(c, &sessione)?;
        let (hash, percorso) = allegati::salva_blob(c, &blob, &f.byte, f.tipo.as_deref())?;
        let mut a = allegati::crea_allegato(c, &sessione, &f.nome, &hash)?;
        let nome = a["nome"].as_str().unwrap_or("file").to_string();
        copia_con_nome(&percorso, &nome)?;
        a["dimensione"] = Value::from(f.byte.len());
        a["tipo_mime"] = json!(f.tipo);
        Ok(a)
    })
    .await
}

fn copia_con_nome(blob: &std::path::Path, nome: &str) -> Risposta<()> {
    let dest = crate::sistema::copia_con_nome(blob, nome);
    if dest.is_file() {
        return Ok(());
    }
    let errore = |e: std::io::Error| Errore::interno(format!("copia dell'allegato: {e}"));
    if let Some(d) = dest.parent() {
        std::fs::create_dir_all(d).map_err(errore)?;
    }
    if std::fs::hard_link(blob, &dest).is_err() {
        std::fs::copy(blob, &dest).map_err(errore)?;
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpoSposta {
    pub macchina_id: String,
}

/// «Continua su…»: solo verso una macchina `connessa` e senza turno in corso.
pub async fn sposta(
    State(s): State<Stato>,
    Path(sessione): Path<String>,
    Corpo(b): Corpo<CorpoSposta>,
) -> RisHttp<Value> {
    if s.motore.occupata(&sessione) {
        return Err(Errore::nuovo(Codice::TurnoInCorso, "c'è un turno in corso").into());
    }
    let sid = sessione.clone();
    let r = json_db(&s, move |c| {
        let (riassunto, seq) = xbot_db::macchine::sposta_sessione(c, &sid, &b.macchina_id)?;
        Ok(json!({
            "sessione": xbot_db::sessioni::leggi(c, &sid)?,
            "riassunto_passaggio": riassunto,
            "aggiornata_seq": seq,
        }))
    })
    .await?;
    if let Some(seq) = r["aggiornata_seq"].as_i64() {
        s.bus.pubblica(
            &sessione,
            Segnale::Righe {
                aggiornata_seq: seq,
            },
        );
    }
    Ok(r)
}
