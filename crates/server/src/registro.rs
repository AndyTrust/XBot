//! I provider caricati dal motore, nell'ordine in cui «auto» li prova.

use std::path::PathBuf;
use std::sync::Arc;

use xbot_core::Risposta;
use xbot_db::{Db, provider as repo};
use xbot_provider::{
    ClaudeCode, CursorAgent, Finto, Grok, Provider, claude_code, cursor_agent, finto, grok,
};

#[derive(Clone)]
pub struct Registro {
    provider: Arc<Vec<Arc<dyn Provider>>>,
}

/// L'adattatore per una riga della tabella `provider`; `None` se l'id non ha adattatore.
pub fn adattatore(
    id: &str,
    programma: &str,
    sessioni: Option<&PathBuf>,
) -> Option<Arc<dyn Provider>> {
    match id {
        finto::ID => Some(Arc::new(Finto::nuovo())),
        claude_code::ID => {
            let mut c = ClaudeCode::nuovo(programma);
            if let Some(s) = sessioni {
                c = c.con_cartella_sessioni(s);
            }
            Some(Arc::new(c))
        }
        // 2026-10-25: Grok e Cursor anche sul server, non solo sui computer collegati.
        // La riga `provider` deve avere `macchina_id` vuoto e il programma della VPS.
        cursor_agent::ID => {
            let mut c = CursorAgent::nuovo(programma);
            if let Some(s) = sessioni {
                c = c.con_cartella_sessioni(s);
            }
            Some(Arc::new(c))
        }
        grok::ID => {
            let mut g = Grok::nuovo(programma);
            if let Some(s) = sessioni {
                g = g.con_cartella_sessioni(s);
                // La casa isolata accanto alle sessioni (`/var/lib/xbot/grok-home` sulla VPS).
                if let Some(dati) = s.parent() {
                    g = g.con_casa(dati.join("grok-home"));
                }
            }
            Some(Arc::new(g))
        }
        _ => None,
    }
}

/// I provider `attivo = 1` della tabella, con il loro adattatore, nell'ordine di «auto».
pub async fn da_tabella(db: &Db, sessioni: Option<PathBuf>) -> Risposta<Vec<Arc<dyn Provider>>> {
    let righe = db.esegui(repo::attivi).await?;
    let mut v = Vec::new();
    for (id, programma) in righe {
        match adattatore(&id, &programma, sessioni.as_ref()) {
            Some(p) => v.push(p),
            None => tracing::warn!(provider = %id, "provider attivo senza adattatore: saltato"),
        }
    }
    if v.is_empty() {
        tracing::warn!("nessun provider attivo: lancia `xbot-server impianta`");
    }
    Ok(v)
}

impl Registro {
    pub fn nuovo(provider: Vec<Arc<dyn Provider>>) -> Self {
        Self {
            provider: Arc::new(provider),
        }
    }

    pub fn trova(&self, id: &str) -> Option<Arc<dyn Provider>> {
        self.provider.iter().find(|p| p.id() == id).cloned()
    }

    pub fn ids(&self) -> Vec<String> {
        self.provider.iter().map(|p| p.id().to_string()).collect()
    }

    /// Scrive provider e modelli nel database (tabelle `provider` e `modello`).
    pub async fn sincronizza(&self, db: &Db) -> Risposta<()> {
        for p in self.provider.iter() {
            let modelli = p.modelli().await?;
            let (id, nome, prog) = (
                p.id().to_string(),
                p.nome().to_string(),
                p.programma().to_string(),
            );
            db.esegui(move |c| {
                repo::registra(c, &id, &nome, &prog)?;
                for (i, m) in modelli.iter().enumerate() {
                    repo::registra_modello(
                        c,
                        &id,
                        &repo::DatiModello {
                            id: &m.id,
                            etichetta: &m.etichetta,
                            dettaglio: m.dettaglio.as_deref(),
                            livelli: &m.livelli,
                            ordine: i64::try_from(i).unwrap_or(0),
                        },
                    )?;
                }
                Ok(())
            })
            .await?;
        }
        Ok(())
    }
}
