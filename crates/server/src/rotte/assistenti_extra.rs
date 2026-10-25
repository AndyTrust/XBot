//! Assistenti, Fase 2.D: avatar (blob), profilo, abilità, memoria, file (Libreria).

use std::path::{Path as Percorso, PathBuf};

use axum::extract::multipart::{Multipart, MultipartRejection};
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use xbot_core::{Codice, Errore, Risposta};
use xbot_db::{allegati, assistenti, impostazioni, tutte_json};

use super::allegati::leggi_file;
use super::json_db;
use crate::errore_http::{Corpo, ErroreHttp, RisHttp};
use crate::profili;
use crate::stato::Stato;

pub const MAX_AVATAR: usize = 2 * 1024 * 1024;

/// Il tipo dell'immagine dai primi byte: png, jpeg o webp.
pub fn tipo_immagine(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

fn imposta(c: &xbot_db::Connessione, id: &str, campo: &str, v: Value) -> Risposta<Value> {
    let mut m = Map::new();
    m.insert(campo.into(), v);
    assistenti::aggiorna(c, id, &m)
}

/// Multipart `immagine` (png/jpg/webp, al massimo 2 MB) → `{avatar_url}`.
pub async fn avatar(
    State(s): State<Stato>,
    Path(id): Path<String>,
    mp: Result<Multipart, MultipartRejection>,
) -> RisHttp<Value> {
    let f = leggi_file(mp, "immagine", MAX_AVATAR).await?;
    let tipo = tipo_immagine(&f.byte).ok_or_else(|| {
        Errore::nuovo(Codice::TipoNonAccettato, "l'immagine vuole png, jpg o webp")
    })?;
    let blob = s.servizi.blob.clone();
    json_db(&s, move |c| {
        assistenti::leggi(c, &id)?;
        let (hash, _) = allegati::salva_blob(c, &blob, &f.byte, Some(tipo))?;
        let a = imposta(c, &id, "avatar_percorso", Value::from(hash))?;
        Ok(json!({ "avatar_url": a["avatar_url"] }))
    })
    .await
}

/// Toglie la foto: l'avatar torna forma e colore.
pub async fn togli_avatar(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    json_db(&s, move |c| {
        imposta(c, &id, "avatar_percorso", Value::Null)?;
        Ok(json!({ "avatar_url": null }))
    })
    .await
}

/// Il contenuto di un blob, con il suo tipo e una cache lunga (il nome è il suo hash).
pub async fn blob(
    State(s): State<Stato>,
    Path(hash): Path<String>,
) -> Result<Response, ErroreHttp> {
    let (percorso, tipo, _) = s.db.esegui(move |c| allegati::blob(c, &hash)).await?;
    let byte = tokio::fs::read(&percorso)
        .await
        .map_err(|_| Errore::nuovo(Codice::NonTrovato, "blob non trovato sul disco"))?;
    Ok((
        [
            (
                header::CONTENT_TYPE,
                tipo.unwrap_or_else(|| "application/octet-stream".into()),
            ),
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable".into(),
            ),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".into()),
        ],
        byte,
    )
        .into_response())
}

/// Il percorso del profilo; se manca lo crea in `<profili>/<id>.md` e lo salva.
fn percorso_profilo(c: &xbot_db::Connessione, dir: &Percorso, id: &str) -> Risposta<PathBuf> {
    let a = assistenti::leggi(c, id)?;
    if let Some(p) = a["profilo_percorso"].as_str().filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    let p = profili::profilo_predefinito(dir, id);
    if !p.is_file() {
        let nome = a["nome"].as_str().unwrap_or("Assistente");
        std::fs::write(&p, format!("# {nome}\n\n"))
            .map_err(|e| Errore::interno(format!("profilo: {e}")))?;
    }
    imposta(
        c,
        id,
        "profilo_percorso",
        Value::from(p.to_string_lossy().into_owned()),
    )?;
    Ok(p)
}

pub async fn profilo(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    let dir = s.servizi.profili.clone();
    json_db(&s, move |c| {
        let p = percorso_profilo(c, &dir, &id)?;
        let testo = std::fs::read_to_string(&p).unwrap_or_default();
        Ok(json!({ "testo": testo, "percorso": p.to_string_lossy() }))
    })
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpoProfilo {
    pub testo: String,
}

pub async fn scrivi_profilo(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Corpo(b): Corpo<CorpoProfilo>,
) -> RisHttp<Value> {
    xbot_db::controlli::lunghezza("testo", Some(&b.testo), profili::MAX_PROFILO)?;
    let dir = s.servizi.profili.clone();
    json_db(&s, move |c| {
        let p = percorso_profilo(c, &dir, &id)?;
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| Errore::interno(format!("profilo: {e}")))?;
        }
        std::fs::write(&p, &b.testo).map_err(|e| Errore::interno(format!("profilo: {e}")))?;
        Ok(json!({ "testo": b.testo, "percorso": p.to_string_lossy() }))
    })
    .await
}

pub async fn abilita(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    let dir = s.servizi.profili.clone();
    json_db(&s, move |c| {
        assistenti::leggi(c, &id)?;
        Ok(json!({ "abilita": profili::elenca(&dir, &id, false)? }))
    })
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NuovaAbilita {
    pub nome: String,
    #[serde(default)]
    pub descrizione: String,
    #[serde(default)]
    pub testo: String,
    #[serde(default = "vero")]
    pub attiva: bool,
}

fn vero() -> bool {
    true
}

pub async fn crea_abilita(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Corpo(b): Corpo<NuovaAbilita>,
) -> RisHttp<profili::Abilita> {
    let dir = s.servizi.profili.clone();
    json_db(&s, move |c| {
        assistenti::leggi(c, &id)?;
        if profili::leggi(&dir, &id, &b.nome).is_ok() {
            return Err(Errore::non_valida(format!(
                "l'abilità «{}» esiste già",
                b.nome
            )));
        }
        profili::scrivi(&dir, &id, &b.nome, &b.descrizione, b.attiva, &b.testo)
    })
    .await
}

pub async fn leggi_abilita(
    State(s): State<Stato>,
    Path((id, nome)): Path<(String, String)>,
) -> RisHttp<profili::Abilita> {
    let dir = s.servizi.profili.clone();
    json_db(&s, move |c| {
        assistenti::leggi(c, &id)?;
        profili::leggi(&dir, &id, &nome)
    })
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModificaAbilita {
    pub attiva: Option<bool>,
    pub testo: Option<String>,
    pub descrizione: Option<String>,
}

pub async fn aggiorna_abilita(
    State(s): State<Stato>,
    Path((id, nome)): Path<(String, String)>,
    Corpo(b): Corpo<ModificaAbilita>,
) -> RisHttp<profili::Abilita> {
    let dir = s.servizi.profili.clone();
    json_db(&s, move |c| {
        assistenti::leggi(c, &id)?;
        let a = profili::leggi(&dir, &id, &nome)?;
        profili::scrivi(
            &dir,
            &id,
            &nome,
            b.descrizione.as_deref().unwrap_or(&a.descrizione),
            b.attiva.unwrap_or(a.attiva),
            b.testo.as_deref().or(a.testo.as_deref()).unwrap_or(""),
        )
    })
    .await
}

pub async fn elimina_abilita(
    State(s): State<Stato>,
    Path((id, nome)): Path<(String, String)>,
) -> Result<StatusCode, ErroreHttp> {
    let dir = s.servizi.profili.clone();
    s.db.esegui(move |c| {
        assistenti::leggi(c, &id)?;
        profili::elimina(&dir, &id, &nome)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Le note `.md` della cartella (fino a due livelli), le più recenti prima, al massimo 50.
fn note(cartella: &Percorso) -> Vec<Value> {
    let mut v: Vec<(i64, String)> = Vec::new();
    let mut coda = vec![(cartella.to_path_buf(), 0)];
    while let Some((dir, livello)) = coda.pop() {
        let Ok(voci) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in voci.flatten() {
            let p = e.path();
            let nome = e.file_name().to_string_lossy().into_owned();
            if nome.starts_with('.') {
                continue;
            }
            if p.is_dir() && livello < 2 {
                coda.push((p, livello + 1));
            } else if p.extension().is_some_and(|x| x == "md") {
                let ms = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0));
                v.push((ms, p.to_string_lossy().into_owned()));
            }
        }
    }
    v.sort_by_key(|a| std::cmp::Reverse(a.0));
    v.into_iter()
        .take(50)
        .map(|(ms, p)| json!({ "percorso": p, "aggiornata_ms": ms }))
        .collect()
}

pub async fn memoria(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    json_db(&s, move |c| {
        let a = assistenti::leggi(c, &id)?;
        let cartella = a["memoria_cartella"].as_str().map(str::to_string);
        // La porta d'ingresso: l'impostazione, altrimenti `Memoria.md` o `<cartella>/<nome>.md`.
        let ingresso = match impostazioni::leggi(c, "memoria.ingresso")? {
            Some(v) if !v.is_null() => Some(v),
            _ => cartella.as_deref().and_then(|d| {
                let d = std::path::Path::new(d);
                let nome = d.file_name().map(|n| format!("{}.md", n.to_string_lossy()));
                ["Memoria.md".to_string()]
                    .into_iter()
                    .chain(nome)
                    .map(|f| d.join(f))
                    .find(|p| p.is_file())
                    .map(|p| Value::from(p.to_string_lossy().into_owned()))
            }),
        };
        let scritture = tutte_json(
            c,
            "SELECT tipo, percorso, ms, sessione_id FROM memoria_scrittura WHERE assistente_id = ?1
             ORDER BY ms DESC LIMIT 50",
            [&id],
        )?;
        Ok(json!({
            "cartella": cartella,
            "ingresso": ingresso,
            "note": cartella.as_deref().map(|c| note(Percorso::new(c))).unwrap_or_default(),
            "scritture": scritture,
        }))
    })
    .await
}

/// La Libreria: allegati delle sessioni e blob prodotti.
pub async fn file(State(s): State<Stato>, Path(id): Path<String>) -> RisHttp<Value> {
    json_db(&s, move |c| {
        Ok(json!({ "file": allegati::dell_assistente(c, &id)? }))
    })
    .await
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn tipi_delle_immagini() {
        assert_eq!(tipo_immagine(b"\x89PNG\r\n\x1a\nxxxx"), Some("image/png"));
        assert_eq!(tipo_immagine(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(tipo_immagine(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(tipo_immagine(b"GIF89a"), None);
    }
}
