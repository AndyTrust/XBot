//! Profili e abilità degli assistenti, nei file.
//!
//! - Profilo: il file di `assistente.profilo_percorso`; se manca nasce in `<profili>/<id>.md`.
//! - Abilità: `<profili>/<id>/abilita/<nome>.md`, con l'intestazione
//!   `---\ndescrizione: …\nattiva: si|no\n---\n` e poi il testo. Un file che inizia con una
//!   riga `descrizione: …` senza intestazione vale come attivo.

use std::path::{Path, PathBuf};

use serde::Serialize;
use xbot_core::{Codice, Errore, Risposta};

pub const MAX_PROFILO: usize = 200_000;
pub const MAX_ABILITA: usize = 100_000;

fn io(p: &Path, e: std::io::Error) -> Errore {
    Errore::interno(format!("{}: {e}", p.display()))
}

/// `<profili>/<id>.md`
pub fn profilo_predefinito(profili: &Path, id: &str) -> PathBuf {
    profili.join(format!("{id}.md"))
}

pub fn cartella_abilita(profili: &Path, id: &str) -> PathBuf {
    profili.join(id).join("abilita")
}

/// Un nome di abilità: minuscole, cifre, `-` e `_`, da 1 a 60 caratteri.
pub fn nome_valido(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 60
        && n.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Abilita {
    pub nome: String,
    pub descrizione: String,
    pub attiva: bool,
    pub percorso: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub testo: Option<String>,
}

/// (descrizione, attiva, testo) dal contenuto di un file di abilità.
pub fn analizza(contenuto: &str) -> (String, bool, String) {
    let mut descrizione = String::new();
    let mut attiva = true;
    if let Some(resto) = contenuto.strip_prefix("---\n")
        && let Some((testa, corpo)) = resto.split_once("\n---\n").or_else(|| {
            resto
                .split_once("\n---")
                .filter(|(_, c)| c.trim().is_empty())
        })
    {
        for riga in testa.lines() {
            if let Some((k, v)) = riga.split_once(':') {
                match k.trim() {
                    "descrizione" => descrizione = v.trim().to_string(),
                    "attiva" => attiva = !matches!(v.trim(), "no" | "false" | "0"),
                    _ => {}
                }
            }
        }
        return (
            descrizione,
            attiva,
            corpo.trim_start_matches('\n').to_string(),
        );
    }
    if let Some((prima, resto)) = contenuto.split_once('\n')
        && let Some(d) = prima.strip_prefix("descrizione:")
    {
        return (d.trim().to_string(), true, resto.to_string());
    }
    (descrizione, attiva, contenuto.to_string())
}

pub fn componi(descrizione: &str, attiva: bool, testo: &str) -> String {
    let d = descrizione.replace('\n', " ");
    format!(
        "---\ndescrizione: {}\nattiva: {}\n---\n{}",
        d.trim(),
        if attiva { "si" } else { "no" },
        testo
    )
}

fn percorso_abilita(profili: &Path, id: &str, nome: &str) -> Risposta<PathBuf> {
    if !nome_valido(nome) {
        return Err(Errore::non_valida(
            "nome dell'abilità: minuscole, cifre, - e _ (al massimo 60)",
        ));
    }
    Ok(cartella_abilita(profili, id).join(format!("{nome}.md")))
}

/// Le abilità di un assistente, in ordine di nome (senza testo, o con `con_testo`).
pub fn elenca(profili: &Path, id: &str, con_testo: bool) -> Risposta<Vec<Abilita>> {
    let dir = cartella_abilita(profili, id);
    let Ok(voci) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut v = Vec::new();
    for e in voci.flatten() {
        let p = e.path();
        let Some(nome) = p.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
            continue;
        };
        if p.extension().is_none_or(|x| x != "md") || !nome_valido(&nome) {
            continue;
        }
        let contenuto = std::fs::read_to_string(&p).map_err(|e| io(&p, e))?;
        let (descrizione, attiva, testo) = analizza(&contenuto);
        v.push(Abilita {
            nome,
            descrizione,
            attiva,
            percorso: p.to_string_lossy().into_owned(),
            testo: con_testo.then_some(testo),
        });
    }
    v.sort_by(|a, b| a.nome.cmp(&b.nome));
    Ok(v)
}

pub fn leggi(profili: &Path, id: &str, nome: &str) -> Risposta<Abilita> {
    let p = percorso_abilita(profili, id, nome)?;
    let contenuto = std::fs::read_to_string(&p)
        .map_err(|_| Errore::nuovo(Codice::NonTrovato, "abilità non trovata"))?;
    let (descrizione, attiva, testo) = analizza(&contenuto);
    Ok(Abilita {
        nome: nome.to_string(),
        descrizione,
        attiva,
        percorso: p.to_string_lossy().into_owned(),
        testo: Some(testo),
    })
}

/// Scrive (crea o sostituisce) un'abilità.
pub fn scrivi(
    profili: &Path,
    id: &str,
    nome: &str,
    descrizione: &str,
    attiva: bool,
    testo: &str,
) -> Risposta<Abilita> {
    xbot_db::controlli::lunghezza("testo", Some(testo), MAX_ABILITA)?;
    xbot_db::controlli::lunghezza(
        "descrizione",
        Some(descrizione),
        xbot_db::controlli::DESCRIZIONE,
    )?;
    let p = percorso_abilita(profili, id, nome)?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    }
    std::fs::write(&p, componi(descrizione, attiva, testo)).map_err(|e| io(&p, e))?;
    leggi(profili, id, nome)
}

pub fn elimina(profili: &Path, id: &str, nome: &str) -> Risposta<()> {
    let p = percorso_abilita(profili, id, nome)?;
    std::fs::remove_file(&p).map_err(|_| Errore::nuovo(Codice::NonTrovato, "abilità non trovata"))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn intestazione_e_file() {
        let (d, a, t) = analizza("---\ndescrizione: Fa i conti\nattiva: no\n---\nTesto\n");
        assert_eq!(
            (d.as_str(), a, t.as_str()),
            ("Fa i conti", false, "Testo\n")
        );
        let (d, a, t) = analizza("descrizione: Breve\ncorpo");
        assert_eq!((d.as_str(), a, t.as_str()), ("Breve", true, "corpo"));
        assert_eq!(analizza("solo testo").2, "solo testo");
        assert_eq!(
            analizza(&componi("x", true, "y")),
            ("x".into(), true, "y".into())
        );
        assert!(nome_valido("conti-2") && !nome_valido("../x") && !nome_valido("Conti"));

        let dir = std::env::temp_dir().join(format!("xbot-abilita-{}", xbot_core::nuovo_id()));
        scrivi(&dir, "a1", "conti", "Fa i conti", true, "Somma tutto").unwrap();
        scrivi(&dir, "a1", "bozze", "Scrive bozze", false, "…").unwrap();
        let v = elenca(&dir, "a1", false).unwrap();
        assert_eq!(
            v.iter().map(|a| a.nome.as_str()).collect::<Vec<_>>(),
            ["bozze", "conti"]
        );
        assert!(!v[0].attiva && v[1].attiva);
        assert!(scrivi(&dir, "a1", "../fuori", "", true, "").is_err());
        elimina(&dir, "a1", "conti").unwrap();
        assert!(leggi(&dir, "a1", "conti").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
