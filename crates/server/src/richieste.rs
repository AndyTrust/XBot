//! Le richieste all'utente viste dal motore: crearle, aspettarne la risposta, risolverle.
//!
//! Chi aspetta (lo strumento `permesso` di S2, `cassaforte_chiedi`…) chiama [`attendi`]; la
//! rotta `POST /richieste/{id}/risolvi` chiama [`risolvi`], che scrive il database e sveglia
//! chi aspetta con un `watch` per richiesta. Lo stato vero sta sempre nel database: chi arriva
//! dopo la risposta la trova lì.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::watch;
use xbot_core::{Errore, Risposta, StatoRichiesta, TipoRichiesta};
use xbot_db::permessi::{self, NuovoPermesso};
use xbot_db::richieste::{self as repo, NuovaRichiesta};

use crate::Stato;
use crate::bus::Segnale;

/// Come è finita una richiesta.
#[derive(Debug, Clone, PartialEq)]
pub struct Risoluzione {
    /// `approvata`, `negata`, `scaduta` o `ritirata`.
    pub esito: StatoRichiesta,
    pub sempre: bool,
    pub comando_approvato: Option<String>,
    /// I valori di un modulo o di un segreto chiesto (mai nei log).
    pub valori: Option<Value>,
}

impl Risoluzione {
    fn da_db(stato: &str, j: &Value) -> Option<Self> {
        let esito = StatoRichiesta::da_testo(stato).ok()?;
        (esito != StatoRichiesta::Aperta).then(|| Self {
            esito,
            sempre: j["sempre"].as_bool().unwrap_or(false),
            comando_approvato: j["comando_approvato"].as_str().map(str::to_string),
            valori: j.get("valori").cloned().filter(|v| !v.is_null()),
        })
    }

    pub fn approvata(&self) -> bool {
        self.esito == StatoRichiesta::Approvata
    }
}

/// Chi sta aspettando quale richiesta.
#[derive(Clone, Default)]
pub struct Attese {
    mappa: Arc<Mutex<HashMap<String, watch::Sender<Option<Risoluzione>>>>>,
}

impl Attese {
    fn mappa(&self) -> MutexGuard<'_, HashMap<String, watch::Sender<Option<Risoluzione>>>> {
        match self.mappa.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        }
    }

    fn iscrivi(&self, id: &str) -> watch::Receiver<Option<Risoluzione>> {
        self.mappa()
            .entry(id.to_string())
            .or_insert_with(|| watch::Sender::new(None))
            .subscribe()
    }

    fn sveglia(&self, id: &str, r: Risoluzione) {
        if let Some(tx) = self.mappa().remove(id) {
            tx.send_replace(Some(r));
        }
    }

    fn dimentica(&self, id: &str) {
        let mut m = self.mappa();
        if m.get(id).is_some_and(|tx| tx.receiver_count() == 0) {
            m.remove(id);
        }
    }
}

/// Il segnale `richiesta {id, tipo, stato}` per il flusso della sessione.
fn segnale_di(r: &Value) -> Segnale {
    let t = |k: &str| r[k].as_str().unwrap_or_default().to_string();
    Segnale::Richiesta {
        id: t("id"),
        tipo: t("tipo"),
        stato: t("stato"),
    }
}

/// Crea una richiesta e la sua riga in chat (con il segnale sul bus). Restituisce la richiesta.
pub async fn crea(
    s: &Stato,
    sessione_id: &str,
    tipo: TipoRichiesta,
    carico: Value,
    scade_tra: Option<Duration>,
) -> Risposta<Value> {
    let n = NuovaRichiesta {
        sessione_id: sessione_id.to_string(),
        tipo,
        carico,
        scade_ms: scade_tra
            .map(|d| xbot_core::ora_ms() + i64::try_from(d.as_millis()).unwrap_or(i64::MAX / 2)),
        piattaforma: None,
    };
    let (r, seq) = s.db.esegui(move |c| repo::crea(c, &n)).await?;
    s.bus.pubblica(sessione_id, segnale_di(&r));
    s.bus.pubblica(
        sessione_id,
        Segnale::Righe {
            aggiornata_seq: seq,
        },
    );
    crate::mcp_interno::figlie::sincronizza(&s.db, &s.bus, sessione_id, false).await;
    tracing::info!(richiesta = %r["id"].as_str().unwrap_or(""), tipo = tipo.come_testo(), "richiesta aperta");
    Ok(r)
}

/// Aspetta la risposta dell'utente fino a `scadenza`. Se il tempo passa, la richiesta diventa
/// `scaduta` (e la riga in chat lo mostra) e la risoluzione ha esito `scaduta`.
pub async fn attendi(s: &Stato, id: &str, scadenza: Duration) -> Risposta<Risoluzione> {
    let mut rx = s.servizi.richieste.iscrivi(id);
    let rid = id.to_string();
    if let Some((stato, j)) = s.db.esegui(move |c| repo::stato(c, &rid)).await? {
        if let Some(r) = Risoluzione::da_db(&stato, &j) {
            s.servizi.richieste.dimentica(id);
            return Ok(r);
        }
    } else {
        s.servizi.richieste.dimentica(id);
        return Err(Errore::nuovo(
            xbot_core::Codice::RichiestaNonTrovata,
            "richiesta non trovata",
        ));
    }
    let attesa = async {
        loop {
            if let Some(r) = rx.borrow_and_update().clone() {
                return Some(r);
            }
            if rx.changed().await.is_err() {
                return None;
            }
        }
    };
    match tokio::time::timeout(scadenza, attesa).await {
        Ok(Some(r)) => Ok(r),
        Ok(None) | Err(_) => {
            s.servizi.richieste.dimentica(id);
            let scaduta = Risoluzione {
                esito: StatoRichiesta::Scaduta,
                sempre: false,
                comando_approvato: None,
                valori: None,
            };
            match chiudi(s, id, &scaduta).await {
                Ok(_) => Ok(scaduta),
                // Risolta proprio adesso: vale la risposta dell'utente.
                Err(e)
                    if matches!(
                        e.codice,
                        xbot_core::Codice::RichiestaGiaRisolta | xbot_core::Codice::RichiestaChiusa
                    ) =>
                {
                    let rid = id.to_string();
                    let st = s.db.esegui(move |c| repo::stato(c, &rid)).await?;
                    Ok(st
                        .and_then(|(stato, j)| Risoluzione::da_db(&stato, &j))
                        .unwrap_or(scaduta))
                }
                Err(e) => Err(e),
            }
        }
    }
}

/// Il corpo di `POST /richieste/{id}/risolvi`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpoRisolvi {
    /// `approvata` | `negata`
    pub esito: String,
    #[serde(default)]
    pub sempre: bool,
    pub comando_approvato: Option<String>,
    pub valori: Option<Value>,
}

async fn chiudi(s: &Stato, id: &str, r: &Risoluzione) -> Risposta<Value> {
    chiudi_con(&s.db, &s.bus, &s.servizi.richieste, id, r).await
}

/// Chiude una richiesta, riscrive la sua riga e sveglia chi aspetta (usabile anche dal motore,
/// che non ha lo `Stato`).
pub async fn chiudi_con(
    db: &xbot_db::Db,
    bus: &crate::bus::Bus,
    attese: &Attese,
    id: &str,
    r: &Risoluzione,
) -> Risposta<Value> {
    let mut j = json!({ "sempre": r.sempre, "comando_approvato": r.comando_approvato });
    if let Some(v) = &r.valori {
        j["valori"] = valori_per_db(v);
    }
    let (rid, esito) = (id.to_string(), r.esito);
    let (ric, seq) = db.esegui(move |c| repo::chiudi(c, &rid, esito, &j)).await?;
    if let (Some(seq), Some(sid)) = (seq, ric["sessione_id"].as_str()) {
        bus.pubblica(
            sid,
            Segnale::Righe {
                aggiornata_seq: seq,
            },
        );
    }
    if let Some(sid) = ric["sessione_id"].as_str() {
        bus.pubblica(sid, segnale_di(&ric));
        crate::mcp_interno::figlie::sincronizza(db, bus, sid, false).await;
    }
    attese.sveglia(id, r.clone());
    Ok(ric)
}

/// I valori come restano nella riga della richiesta: solo `scelta` (domanda) e `salva` in
/// chiaro, il resto (password, token, campi dei moduli) diventa `***`. I valori veri arrivano
/// solo a chi aspetta, in memoria.
fn valori_per_db(v: &Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| {
                    let tieni = matches!(k.as_str(), "scelta" | "salva");
                    (
                        k.clone(),
                        if tieni { x.clone() } else { Value::from("***") },
                    )
                })
                .collect(),
        ),
        _ => Value::from("***"),
    }
}

/// Ritira una richiesta aperta (turno interrotto o finito): esito `ritirata`.
pub async fn ritira(
    db: &xbot_db::Db,
    bus: &crate::bus::Bus,
    attese: &Attese,
    id: &str,
) -> Risposta<Value> {
    let r = Risoluzione {
        esito: StatoRichiesta::Ritirata,
        sempre: false,
        comando_approvato: None,
        valori: None,
    };
    chiudi_con(db, bus, attese, id, &r).await
}

/// Risolve una richiesta aperta. Con `sempre` e `approvata` scrive anche la concessione
/// stabile (`permesso_azione`) dal carico: `azione` (predefinita dal tipo), `destinatario`,
/// `risorsa` (o `comando_approvato`, o `comando`).
pub async fn risolvi(s: &Stato, id: &str, corpo: CorpoRisolvi) -> Risposta<Value> {
    let esito = match corpo.esito.as_str() {
        "approvata" => StatoRichiesta::Approvata,
        "negata" => StatoRichiesta::Negata,
        _ => return Err(Errore::non_valida("esito vuole approvata o negata")),
    };
    let r = Risoluzione {
        esito,
        sempre: corpo.sempre,
        comando_approvato: corpo.comando_approvato,
        valori: corpo.valori,
    };
    let ric = chiudi(s, id, &r).await?;
    if r.sempre {
        let carico = ric["carico_json"].clone();
        let tipo = ric["tipo"].as_str().unwrap_or("").to_string();
        let sid = ric["sessione_id"].as_str().unwrap_or("").to_string();
        let aid = ric["assistente_id"].as_str().map(str::to_string);
        let comando = r.comando_approvato.clone();
        let concesso = if r.approvata() { "sempre" } else { "mai" };
        s.db.esegui(move |c| {
            let macchina = xbot_db::sessioni::leggi(c, &sid)?.macchina_id;
            let testo = |k: &str| carico[k].as_str().map(str::to_string);
            permessi::concedi(
                c,
                &NuovoPermesso {
                    macchina_id: macchina,
                    assistente_id: aid,
                    azione: testo("azione").unwrap_or_else(|| match tipo.as_str() {
                        "permesso_locale" | "revisione_comando" => "run-command".into(),
                        altro => altro.to_string(),
                    }),
                    destinatario: testo("destinatario").or_else(|| testo("strumento")),
                    risorsa: comando
                        .or_else(|| testo("risorsa"))
                        .or_else(|| testo("comando")),
                    concesso: concesso.into(),
                },
            )
        })
        .await?;
    }
    Ok(ric)
}
