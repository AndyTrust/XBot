//! Traduce gli `EventoTurno` del provider in righe di trascrizione.

use std::collections::HashMap;

use serde_json::{Value, json};
use xbot_core::{GenereRiga, Risposta, Ruolo};
use xbot_db::{Db, provider, righe, sessioni};
use xbot_provider::EventoTurno;

use crate::bus::{Bus, Segnale};

/// Come è finito il flusso di eventi.
#[derive(Debug, Clone, PartialEq)]
pub enum Fine {
    Finito {
        token_ingresso: Option<i64>,
        token_uscita: Option<i64>,
        token_cache: Option<i64>,
        /// In dollari, come lo dichiara il client.
        costo_usd: Option<f64>,
    },
    Fallito {
        codice: String,
        testo: String,
    },
}

/// Lo stato della traduzione di un turno.
pub struct Traduttore {
    db: Db,
    bus: Bus,
    sessione_id: String,
    turno_id: String,
    provider_id: String,
    /// L'ultima lettura di ogni finestra della quota (`five_hour`, `seven_day`).
    finestre: serde_json::Map<String, Value>,
    riga_testo: Option<String>,
    testo: String,
    riga_ragionamento: Option<String>,
    ragionamento: String,
    strumenti: HashMap<String, (String, Value)>,
}

impl Traduttore {
    pub fn nuovo(db: Db, bus: Bus, sessione_id: &str, turno_id: &str, provider_id: &str) -> Self {
        Self {
            db,
            bus,
            sessione_id: sessione_id.to_string(),
            turno_id: turno_id.to_string(),
            provider_id: provider_id.to_string(),
            finestre: serde_json::Map::new(),
            riga_testo: None,
            testo: String::new(),
            riga_ragionamento: None,
            ragionamento: String::new(),
            strumenti: HashMap::new(),
        }
    }

    async fn aggiungi(&self, genere: GenereRiga, ruolo: Ruolo, corpo: Value) -> Risposta<String> {
        let sid = self.sessione_id.clone();
        let r = self
            .db
            .esegui(move |c| righe::aggiungi(c, &sid, genere, Some(ruolo), &corpo))
            .await?;
        self.bus.pubblica(
            &self.sessione_id,
            Segnale::Righe {
                aggiornata_seq: r.aggiornata_seq,
            },
        );
        Ok(r.id)
    }

    async fn riscrivi(&self, riga_id: &str, corpo: Value) -> Risposta<()> {
        let rid = riga_id.to_string();
        let r = self
            .db
            .esegui(move |c| righe::riscrivi(c, &rid, &corpo))
            .await?;
        self.bus.pubblica(
            &self.sessione_id,
            Segnale::Righe {
                aggiornata_seq: r.aggiornata_seq,
            },
        );
        Ok(())
    }

    /// Il testo dell'assistente: la stessa riga riscritta mentre arriva.
    async fn scrivi_testo(&mut self) -> Risposta<()> {
        let corpo = json!({ "testo": self.testo, "turno_id": self.turno_id });
        match &self.riga_testo {
            Some(id) => self.riscrivi(id, corpo).await,
            None => {
                let id = self
                    .aggiungi(GenereRiga::Messaggio, Ruolo::Assistente, corpo)
                    .await?;
                self.riga_testo = Some(id);
                Ok(())
            }
        }
    }

    /// Applica un evento. Restituisce `Some(Fine)` sull'evento che chiude il turno.
    pub async fn applica(&mut self, evento: EventoTurno) -> Risposta<Option<Fine>> {
        match evento {
            EventoTurno::Avviato {
                sessione_esterna_id,
            } => self.ricorda_esterna(sessione_esterna_id).await?,
            EventoTurno::TestoDelta(d) => {
                self.testo.push_str(&d);
                self.scrivi_testo().await?;
            }
            EventoTurno::TestoCompleto(t) => {
                self.testo = t;
                self.scrivi_testo().await?;
            }
            EventoTurno::Ragionamento(t) => {
                self.ragionamento.push_str(&t);
                let corpo = json!({ "testo": self.ragionamento, "turno_id": self.turno_id });
                match &self.riga_ragionamento {
                    Some(id) => self.riscrivi(id, corpo).await?,
                    None => {
                        let id = self
                            .aggiungi(GenereRiga::Ragionamento, Ruolo::Assistente, corpo)
                            .await?;
                        self.riga_ragionamento = Some(id);
                    }
                }
            }
            EventoTurno::ChiamataStrumento { nome, input_json } => {
                let corpo = json!({
                    "nome": nome, "input": input_json, "stato": "in_corso", "turno_id": self.turno_id
                });
                let id = self
                    .aggiungi(
                        GenereRiga::ChiamataStrumento,
                        Ruolo::Assistente,
                        corpo.clone(),
                    )
                    .await?;
                self.strumenti.insert(nome, (id, corpo));
                // Il testo dopo uno strumento va in una bolla nuova.
                self.riga_testo = None;
                self.testo.clear();
            }
            EventoTurno::EsitoStrumento {
                nome,
                esito_json,
                ok,
            } => match self.strumenti.remove(&nome) {
                Some((id, mut corpo)) => {
                    corpo["esito"] = esito_json;
                    corpo["ok"] = Value::Bool(ok);
                    corpo["stato"] = Value::from(if ok { "riuscito" } else { "fallito" });
                    self.riscrivi(&id, corpo).await?;
                }
                None => {
                    let corpo = json!({ "nome": nome, "esito": esito_json, "ok": ok });
                    self.aggiungi(GenereRiga::Esito, Ruolo::Assistente, corpo)
                        .await?;
                }
            },
            EventoTurno::Avviso(t) => {
                self.aggiungi(GenereRiga::Avviso, Ruolo::Sistema, json!({ "testo": t }))
                    .await?;
            }
            EventoTurno::Finito {
                token_ingresso,
                token_uscita,
                token_cache,
                costo_usd,
                sessione_esterna_id,
            } => {
                self.ricorda_esterna(sessione_esterna_id).await?;
                return Ok(Some(Fine::Finito {
                    token_ingresso,
                    token_uscita,
                    token_cache,
                    costo_usd,
                }));
            }
            EventoTurno::Quota {
                finestra,
                utilizzo,
                azzera_ms,
            } => self.quota(finestra, utilizzo, azzera_ms).await?,
            EventoTurno::Fallito { codice, testo } => {
                self.avviso_errore(&codice, &testo).await?;
                return Ok(Some(Fine::Fallito { codice, testo }));
            }
        }
        Ok(None)
    }

    /// Una riga `avviso` con il codice dell'errore.
    pub async fn avviso_errore(&self, codice: &str, testo: &str) -> Risposta<()> {
        let corpo = json!({ "codice": codice, "testo": testo, "turno_id": self.turno_id });
        self.aggiungi(GenereRiga::Avviso, Ruolo::Sistema, corpo)
            .await?;
        Ok(())
    }

    /// Ogni finestra si ricorda; la `seven_day` è l'anello e scrive una `quota_rilevazione`
    /// (fonte `client`) con tutte le finestre lette finora in `dettaglio_json`.
    async fn quota(
        &mut self,
        finestra: String,
        utilizzo: f64,
        azzera_ms: Option<i64>,
    ) -> Risposta<()> {
        let percentuale = utilizzo * 100.0;
        self.finestre.insert(
            finestra.clone(),
            json!({ "percentuale_usata": percentuale, "azzera_ms": azzera_ms }),
        );
        if finestra != "seven_day" {
            return Ok(());
        }
        let (pid, dettaglio) = (
            self.provider_id.clone(),
            Value::Object(self.finestre.clone()),
        );
        self.db
            .esegui(move |c| {
                provider::rileva_quota(c, &pid, percentuale, azzera_ms, "client", Some(&dettaglio))
            })
            .await
    }

    async fn ricorda_esterna(&self, esterna: Option<String>) -> Risposta<()> {
        if let Some(e) = esterna {
            let sid = self.sessione_id.clone();
            self.db
                .esegui(move |c| sessioni::imposta_esterna(c, &sid, &e))
                .await?;
        }
        Ok(())
    }
}
