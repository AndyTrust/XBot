//! Il giornale locale dei lavori: `<cartella>/giornale/<lavoro_id>.jsonl`, una riga per frame
//! (`preso`, `evento`, `richiesta`, `chiuso`) con il suo `seq`. Il socket può cadere: alla
//! riconnessione si rimanda tutto quello che il server non ha confermato con `ok`. Il file si
//! cancella quando il server ha confermato il `chiuso`.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use xbot_core::{Errore, Risposta};

use crate::protocollo::{Corpo, DalLavoratore};

#[derive(Debug, Default)]
struct Voci {
    prossimo: u64,
    confermato: u64,
    righe: Vec<(u64, String)>,
    chiuso: bool,
}

#[derive(Debug)]
pub struct Giornale {
    cartella: PathBuf,
    lavori: BTreeMap<String, Voci>,
}

/// Un id di lavoro usabile come nome di file.
fn id_valido(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl Giornale {
    /// Apre la cartella e rilegge i giornali rimasti (da un'esecuzione precedente).
    pub fn apri(cartella: &Path) -> Risposta<Self> {
        let cartella = cartella.join("giornale");
        std::fs::create_dir_all(&cartella)
            .map_err(|e| Errore::interno(format!("{}: {e}", cartella.display())))?;
        let mut lavori = BTreeMap::new();
        let elenco = std::fs::read_dir(&cartella)
            .map_err(|e| Errore::interno(format!("{}: {e}", cartella.display())))?;
        for voce in elenco.flatten() {
            let p = voce.path();
            if p.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(id) = p.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
                continue;
            };
            let testo = std::fs::read_to_string(&p).unwrap_or_default();
            let mut v = Voci::default();
            for riga in testo.lines().filter(|r| !r.trim().is_empty()) {
                let Ok(f) = serde_json::from_str::<DalLavoratore>(riga) else {
                    continue;
                };
                if let Some((_, seq)) = f.lavoro_seq() {
                    v.prossimo = v.prossimo.max(seq);
                    v.chiuso |= matches!(f, DalLavoratore::Chiuso { .. });
                    v.righe.push((seq, riga.to_string()));
                }
            }
            lavori.insert(id, v);
        }
        Ok(Self { cartella, lavori })
    }

    fn file(&self, id: &str) -> PathBuf {
        self.cartella.join(format!("{id}.jsonl"))
    }

    /// Aggiunge un frame (gli dà il `seq` e lo scrive su disco). Restituisce la riga JSON.
    pub fn aggiungi(&mut self, lavoro_id: &str, corpo: Corpo) -> Risposta<String> {
        if !id_valido(lavoro_id) {
            return Err(Errore::non_valida(format!(
                "id di lavoro non valido: {lavoro_id:?}"
            )));
        }
        let chiude = matches!(corpo, Corpo::Chiuso { .. });
        let v = self.lavori.entry(lavoro_id.to_string()).or_default();
        v.prossimo += 1;
        let seq = v.prossimo;
        let riga = serde_json::to_string(&corpo.in_frame(lavoro_id, seq))
            .map_err(|e| Errore::interno(e.to_string()))?;
        v.righe.push((seq, riga.clone()));
        v.chiuso |= chiude;
        let file = self.file(lavoro_id);
        let scritto = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file)
            .and_then(|mut f| f.write_all(format!("{riga}\n").as_bytes()));
        if let Err(e) = scritto {
            // Il frame resta in memoria: si perde solo se il lavoratore si ferma.
            tracing::warn!(file = %file.display(), errore = %e, "giornale non scritto");
        }
        Ok(riga)
    }

    /// Il server ha tutto fino a `fino_a` compreso.
    pub fn conferma(&mut self, lavoro_id: &str, fino_a: u64) {
        let file = self.file(lavoro_id);
        let Some(v) = self.lavori.get_mut(lavoro_id) else {
            return;
        };
        if fino_a <= v.confermato {
            return;
        }
        v.confermato = fino_a;
        v.righe.retain(|(s, _)| *s > fino_a);
        if v.chiuso && v.righe.is_empty() {
            self.lavori.remove(lavoro_id);
            let _ = std::fs::remove_file(&file);
            return;
        }
        let resto: String = v.righe.iter().map(|(_, r)| format!("{r}\n")).collect();
        if let Err(e) = std::fs::write(&file, resto) {
            tracing::warn!(file = %file.display(), errore = %e, "giornale non riscritto");
        }
    }

    /// Le righe da rimandare alla riconnessione, in ordine.
    pub fn da_rimandare(&self) -> Vec<String> {
        self.lavori
            .values()
            .flat_map(|v| v.righe.iter().map(|(_, r)| r.clone()))
            .collect()
    }

    /// I lavori del giornale non ancora chiusi.
    pub fn aperti(&self) -> Vec<String> {
        self.lavori
            .iter()
            .filter(|(_, v)| !v.chiuso)
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Il lavoro è noto (in corso o in attesa di conferma).
    pub fn conosce(&self, lavoro_id: &str) -> bool {
        self.lavori.contains_key(lavoro_id)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::protocollo::EventoFilo;

    #[test]
    fn aggiungi_conferma_rileggi() {
        let d = std::env::temp_dir().join(format!("xbot-giornale-{}", xbot_core::nuovo_id()));
        let mut g = Giornale::apri(&d).unwrap();
        g.aggiungi("L1", Corpo::Preso { generazione: 1 }).unwrap();
        g.aggiungi(
            "L1",
            Corpo::Evento(EventoFilo::TestoDelta { testo: "a".into() }),
        )
        .unwrap();
        assert!(g.aggiungi("../x", Corpo::Preso { generazione: 1 }).is_err());
        g.conferma("L1", 1);
        assert_eq!(g.da_rimandare().len(), 1);
        // Riaperto: si ritrova il resto, e il seq riparte da dove era.
        let mut g = Giornale::apri(&d).unwrap();
        assert_eq!(g.aperti(), vec!["L1".to_string()]);
        let r = g
            .aggiungi(
                "L1",
                Corpo::Chiuso {
                    stato: "riuscito".into(),
                    motivo: None,
                },
            )
            .unwrap();
        assert!(r.contains("\"seq\":3"), "{r}");
        g.conferma("L1", 3);
        assert!(!g.conosce("L1"));
        assert!(!d.join("giornale/L1.jsonl").exists());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
