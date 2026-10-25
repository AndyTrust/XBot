//! Il canale dei computer (2.E S-B): `GET /api/v1/macchine/{id}/canale`, WebSocket con la credenziale
//! del lavoratore (`Authorization: Bearer …`); frame JSON con `tipo` (sez. 8 di `CONTRATTO-2E.md`).
//!
//! L'`Hub` tiene i socket aperti e i lavori vivi (il turno che aspetta gli eventi). Il socket chiuso
//! non annulla il turno: il lavoratore rimanda gli eventi del giornale con il loro `seq`, il server
//! salva ogni `seq` una volta sola (`lavoro_evento`) e risponde `ok`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use xbot_core::{Codice, Errore, StatoLavoro, TipoRichiesta, ora_ms};
use xbot_db::provider::ProviderDichiarato;
use xbot_db::{eventi, lavori, macchine};
use xbot_provider::EventoTurno;

use crate::Stato;
use crate::errore_http::ErroreHttp;

#[path = "canale_macchina_permessi.rs"]
mod permessi_mac;

/// I tempi del canale (modificabili nelle prove).
#[derive(Debug, Clone, Copy)]
pub struct Tempi {
    /// Lease dei lavori presi: ultimo battito + questo (60 s).
    pub lease_ms: i64,
    /// Dopo questo tempo senza battito un lavoro preso fallisce `macchina_persa` (10 minuti).
    pub persa_ms: i64,
    /// Quanto aspetta un lavoro in coda che il computer si ricolleghi (24 ore).
    pub attesa_max_ms: i64,
    /// Dopo quanto un lavoro in attesa manda la push (5 minuti).
    pub push_dopo_ms: i64,
    /// Ogni quanto gira il sorvegliante.
    pub giro_ms: u64,
}

impl Default for Tempi {
    fn default() -> Self {
        Self {
            lease_ms: 60_000,
            persa_ms: 10 * 60_000,
            attesa_max_ms: 24 * 3_600_000,
            push_dopo_ms: 5 * 60_000,
            giro_ms: 5_000,
        }
    }
}

/// Un lavoro vivo: il turno lo aspetta sul canale `eventi`.
pub struct Vivo {
    pub macchina_id: String,
    pub sessione_id: String,
    pub turno_id: String,
    /// Il frame `lavoro` (con i segreti: solo in memoria).
    pub frame: String,
    /// Il lavoratore ha detto `preso`.
    pub preso: bool,
    pub eventi: mpsc::Sender<EventoTurno>,
    /// «Esegui sulla VPS»: sveglia il turno in attesa perché riparta sul server.
    pub sulla_vps: Arc<tokio::sync::Notify>,
}

struct Collegato {
    conn: u64,
    tx: mpsc::UnboundedSender<String>,
}

#[derive(Default)]
struct Mappe {
    connessi: HashMap<String, Collegato>,
    lavori: HashMap<String, Vivo>,
    /// L'ultimo battito visto per macchina (ms).
    battiti: HashMap<String, i64>,
    /// I posti liberi dichiarati nell'ultimo battito.
    posti_liberi: HashMap<String, i64>,
    prossimo: u64,
}

#[derive(Default)]
struct InternoHub {
    mappe: Mutex<Mappe>,
    tempi: Mutex<Tempi>,
    server_id: OnceLock<String>,
    stato: OnceLock<Weak<crate::stato::Interno>>,
}

/// I computer collegati e i loro lavori.
#[derive(Clone, Default)]
pub struct Hub(Arc<InternoHub>);

impl Hub {
    fn mappe(&self) -> MutexGuard<'_, Mappe> {
        match self.0.mappe.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        }
    }

    pub fn tempi(&self) -> Tempi {
        match self.0.tempi.lock() {
            Ok(g) => *g,
            Err(e) => *e.into_inner(),
        }
    }

    /// Per le prove: tempi corti.
    pub fn imposta_tempi(&self, t: Tempi) {
        match self.0.tempi.lock() {
            Ok(mut g) => *g = t,
            Err(e) => *e.into_inner() = t,
        }
    }

    pub fn imposta_server(&self, id: &str) {
        let _ = self.0.server_id.set(id.to_string());
    }

    pub fn server_id(&self) -> String {
        self.0.server_id.get().cloned().unwrap_or_default()
    }

    pub fn imposta_stato(&self, s: &Stato) {
        let _ = self.0.stato.set(Arc::downgrade(&s.0));
    }

    pub fn stato_server(&self) -> Option<Stato> {
        self.0.stato.get().and_then(Weak::upgrade).map(Stato)
    }

    /// Le macchine con il canale aperto adesso.
    pub fn connesse(&self) -> Vec<String> {
        self.mappe().connessi.keys().cloned().collect()
    }

    /// I posti liberi dell'ultimo battito di una macchina.
    pub fn posti_liberi(&self, id: &str) -> Option<i64> {
        self.mappe().posti_liberi.get(id).copied()
    }

    pub fn e_connessa(&self, id: &str) -> bool {
        self.mappe().connessi.contains_key(id)
    }

    /// Manda un frame a una macchina collegata. Falso se non è collegata.
    pub fn manda(&self, macchina_id: &str, frame: &Value) -> bool {
        self.mappe()
            .connessi
            .get(macchina_id)
            .is_some_and(|c| c.tx.send(frame.to_string()).is_ok())
    }

    pub fn registra(&self, lavoro_id: &str, v: Vivo) {
        self.mappe().lavori.insert(lavoro_id.to_string(), v);
    }

    /// Spedisce il frame `lavoro` se la macchina è collegata.
    pub fn spedisci(&self, lavoro_id: &str) -> bool {
        let m = self.mappe();
        let Some(v) = m.lavori.get(lavoro_id) else {
            return false;
        };
        m.connessi
            .get(&v.macchina_id)
            .is_some_and(|c| c.tx.send(v.frame.clone()).is_ok())
    }

    /// «Esegui sulla VPS» per il turno dato: solo se il suo lavoro aspetta ancora il computer
    /// (non preso), altrimenti 409 `turno_in_corso`.
    pub fn sulla_vps(&self, turno_id: &str) -> xbot_core::Risposta<()> {
        let m = self.mappe();
        match m
            .lavori
            .values()
            .find(|v| v.turno_id == turno_id && !v.preso)
        {
            Some(v) => {
                v.sulla_vps.notify_one();
                Ok(())
            }
            None => Err(Errore::nuovo(
                Codice::TurnoInCorso,
                "il turno non è in attesa di un computer",
            )),
        }
    }

    pub fn preso(&self, lavoro_id: &str) -> bool {
        self.mappe().lavori.get(lavoro_id).is_some_and(|v| v.preso)
    }

    pub fn togli(&self, lavoro_id: &str) -> Option<Vivo> {
        self.mappe().lavori.remove(lavoro_id)
    }

    /// Ferma un lavoro: frame `interrompi` (se la macchina c'è) e fine del flusso degli eventi.
    pub fn interrompi(&self, lavoro_id: &str) {
        if let Some(v) = self.togli(lavoro_id) {
            self.manda(
                &v.macchina_id,
                &json!({ "tipo": "interrompi", "lavoro_id": lavoro_id }),
            );
        }
    }

    /// Il mittente degli eventi di un lavoro di questa macchina.
    fn mittente(&self, macchina_id: &str, lavoro_id: &str) -> Option<mpsc::Sender<EventoTurno>> {
        self.mappe()
            .lavori
            .get(lavoro_id)
            .filter(|v| v.macchina_id == macchina_id)
            .map(|v| v.eventi.clone())
    }

    fn segna_preso(&self, lavoro_id: &str) {
        if let Some(v) = self.mappe().lavori.get_mut(lavoro_id) {
            v.preso = true;
        }
    }

    fn battito(&self, macchina_id: &str) {
        self.mappe()
            .battiti
            .insert(macchina_id.to_string(), ora_ms());
    }

    /// Registra il socket (sostituisce un eventuale socket vecchio) e rimanda i lavori non
    /// ancora presi. Restituisce il numero della connessione.
    fn collega(&self, macchina_id: &str, tx: mpsc::UnboundedSender<String>) -> u64 {
        let mut m = self.mappe();
        m.prossimo += 1;
        let conn = m.prossimo;
        for v in m.lavori.values() {
            if v.macchina_id == macchina_id && !v.preso {
                let _ = tx.send(v.frame.clone());
            }
        }
        m.connessi
            .insert(macchina_id.to_string(), Collegato { conn, tx });
        m.battiti.insert(macchina_id.to_string(), ora_ms());
        conn
    }

    /// Toglie il socket se è ancora quello. Vero se la macchina ha lavori vivi.
    fn scollega(&self, macchina_id: &str, conn: u64) -> Option<bool> {
        let mut m = self.mappe();
        if m.connessi.get(macchina_id).map(|c| c.conn) != Some(conn) {
            return None;
        }
        m.connessi.remove(macchina_id);
        Some(m.lavori.values().any(|v| v.macchina_id == macchina_id))
    }

    /// Chiude il socket di una macchina (rimossa).
    pub fn caccia(&self, macchina_id: &str) {
        self.mappe().connessi.remove(macchina_id);
        let lavori: Vec<String> = self
            .mappe()
            .lavori
            .iter()
            .filter(|(_, v)| v.macchina_id == macchina_id)
            .map(|(k, _)| k.clone())
            .collect();
        for l in lavori {
            if let Some(v) = self.togli(&l) {
                let _ = v.eventi.try_send(EventoTurno::Fallito {
                    codice: "macchina_rimossa".into(),
                    testo: "il computer è stato rimosso".into(),
                });
            }
        }
    }
}

// ───────────────────────── frame dal lavoratore ─────────────────────────

#[derive(Debug, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
enum DalLavoratore {
    Battito {
        #[serde(default)]
        posti_liberi: Option<i64>,
        #[serde(default)]
        provider: Vec<ProviderDichiarato>,
        #[serde(default)]
        lavori: Vec<String>,
    },
    Preso {
        lavoro_id: String,
        generazione: i64,
        seq: i64,
    },
    Evento {
        lavoro_id: String,
        seq: i64,
        evento: Value,
    },
    Richiesta {
        lavoro_id: String,
        seq: i64,
        richiesta_id: String,
        /// Il tipo della richiesta (`permesso_locale`, `revisione_comando`, …): non può
        /// chiamarsi `tipo`, che è il tipo del frame.
        #[serde(default)]
        tipo_richiesta: Option<String>,
        #[serde(default)]
        carico: Value,
    },
    Chiuso {
        lavoro_id: String,
        seq: i64,
        stato: String,
        #[serde(default)]
        motivo: Option<String>,
    },
    /// 2.I: `accedi`/`esci` (`richiesta_id`, `provider_id`, `stato`, …); 2.K: `esito_prova`.
    #[serde(alias = "esito_prova")]
    Accesso {
        #[serde(flatten)]
        campi: serde_json::Map<String, Value>,
    },
}

/// Un `EventoTurno` dal filo: `{"tipo": "testo_delta", "testo": "…"}` (nomi della sezione 8).
pub fn evento_da_filo(v: &Value) -> Option<EventoTurno> {
    let t = |k: &str| v[k].as_str().map(str::to_string);
    let n = |k: &str| v[k].as_i64();
    Some(match v["tipo"].as_str()? {
        "avviato" => EventoTurno::Avviato {
            sessione_esterna_id: t("sessione_esterna_id"),
        },
        "testo_delta" => EventoTurno::TestoDelta(t("testo")?),
        "testo_completo" => EventoTurno::TestoCompleto(t("testo")?),
        "ragionamento" => EventoTurno::Ragionamento(t("testo")?),
        "chiamata_strumento" => EventoTurno::ChiamataStrumento {
            nome: t("nome")?,
            input_json: v["input_json"].clone(),
        },
        "esito_strumento" => EventoTurno::EsitoStrumento {
            nome: t("nome")?,
            esito_json: v["esito_json"].clone(),
            ok: v["ok"].as_bool().unwrap_or(false),
        },
        "avviso" => EventoTurno::Avviso(t("testo")?),
        "finito" => EventoTurno::Finito {
            token_ingresso: n("token_ingresso"),
            token_uscita: n("token_uscita"),
            token_cache: n("token_cache"),
            costo_usd: v["costo_usd"].as_f64(),
            sessione_esterna_id: t("sessione_esterna_id"),
        },
        "quota" => EventoTurno::Quota {
            finestra: t("finestra")?,
            utilizzo: v["utilizzo"].as_f64()?,
            azzera_ms: n("azzera_ms"),
        },
        "fallito" => EventoTurno::Fallito {
            codice: t("codice").unwrap_or_else(|| "lavoratore_fallito".into()),
            testo: t("testo").unwrap_or_default(),
        },
        _ => return None,
    })
}

fn bearer(h: &HeaderMap) -> Option<String> {
    h.get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// `GET /api/v1/macchine/{id}/canale`: 401 senza credenziale valida per quella macchina.
pub async fn canale(
    State(s): State<Stato>,
    Path(id): Path<String>,
    h: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let Some(cred) = bearer(&h) else {
        return ErroreHttp(Errore::nuovo(
            Codice::NonAutorizzato,
            "manca la credenziale",
        ))
        .into_response();
    };
    let hash = crate::accesso::hash_token(&cred);
    let trovata =
        s.db.esegui(move |c| macchine::per_credenziale(c, &hash))
            .await;
    match trovata {
        Ok(Some((mid, _))) if mid == id => ws.on_upgrade(move |sock| sessione(s, id, sock)),
        Ok(_) => ErroreHttp(Errore::nuovo(
            Codice::NonAutorizzato,
            "credenziale non valida per questa macchina",
        ))
        .into_response(),
        Err(e) => ErroreHttp(e).into_response(),
    }
}

async fn sessione(s: Stato, macchina_id: String, sock: WebSocket) {
    let hub = s.motore.macchine().clone();
    let (mut scrivi, mut leggi) = sock.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let mid = macchina_id.clone();
    let aperti =
        s.db.esegui(move |c| {
            macchine::battito(c, &mid)?;
            eventi::scrivi(c, "macchina_collegata", &json!({ "macchina_id": mid }))?;
            lavori::aperti(c, &mid)
        })
        .await
        .unwrap_or_default();
    let ciao = json!({
        "tipo": "ciao", "macchina_id": macchina_id, "server_versione": xbot_core::VERSIONE,
        "lavori_aperti": aperti.iter().map(|l| json!({
            "lavoro_id": l["id"], "fino_a_seq": l["fino_a_seq"], "stato": l["stato"]
        })).collect::<Vec<_>>(),
    });
    let _ = tx.send(ciao.to_string());
    let conn = hub.collega(&macchina_id, tx.clone());
    tracing::info!(macchina = %macchina_id, "computer collegato");
    // Dopo i `lavoro` aperti (mandati da `collega`), le `risolta` rimaste in coda.
    consegna_risolte(&s, &macchina_id).await;
    let scrittore = tokio::spawn(async move {
        while let Some(t) = rx.recv().await {
            if scrivi.send(Message::Text(t.into())).await.is_err() {
                break;
            }
        }
        let _ = scrivi.close().await;
    });
    while let Some(m) = leggi.next().await {
        let testo = match m {
            Ok(Message::Text(t)) => t.to_string(),
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => continue,
        };
        if !hub.e_connessa(&macchina_id) {
            break; // rimossa o sostituita da un socket nuovo
        }
        match serde_json::from_str::<DalLavoratore>(&testo) {
            Ok(f) => frame(&s, &macchina_id, f, &tx).await,
            Err(e) => {
                let _ = tx.send(
                    json!({ "tipo": "errore", "codice": "frame_non_valido", "testo": e.to_string() })
                        .to_string(),
                );
            }
        }
    }
    scrittore.abort();
    if let Some(ha_lavori) = hub.scollega(&macchina_id, conn) {
        let stato = if ha_lavori { "riconnessione" } else { "ferma" };
        let mid = macchina_id.clone();
        let _ =
            s.db.esegui(move |c| {
                macchine::imposta_stato(c, &mid, stato)?;
                eventi::scrivi(
                    c,
                    "macchina_persa",
                    &json!({ "macchina_id": mid, "motivo": "socket_chiuso", "stato": stato }),
                )
            })
            .await;
        tracing::info!(macchina = %macchina_id, stato, "computer scollegato");
    }
}

/// `ok {lavoro_id, fino_a_seq}` dopo aver salvato il frame.
fn conferma(tx: &mpsc::UnboundedSender<String>, lavoro_id: &str, fino_a_seq: i64) {
    let _ = tx.send(
        json!({ "tipo": "ok", "lavoro_id": lavoro_id, "fino_a_seq": fino_a_seq }).to_string(),
    );
}

/// Salva il frame di un lavoro (una volta per `seq`). `None` se il lavoro non è di questa
/// macchina; altrimenti (nuovo, fino_a_seq).
async fn salva(
    s: &Stato,
    macchina_id: &str,
    lavoro_id: &str,
    seq: i64,
    tipo: &'static str,
    dati: Value,
) -> Option<(bool, i64)> {
    let (mid, lid) = (macchina_id.to_string(), lavoro_id.to_string());
    s.db.esegui(move |c| {
        let l = match lavori::leggi(c, &lid) {
            Ok(l) => l,
            Err(_) => return Ok(None),
        };
        if l["macchina_id"].as_str() != Some(mid.as_str()) {
            return Ok(None);
        }
        let nuovo = lavori::registra_evento(c, &lid, seq, tipo, &dati)?;
        Ok(Some((nuovo, lavori::fino_a_seq(c, &lid)?)))
    })
    .await
    .ok()
    .flatten()
}

async fn frame(s: &Stato, macchina_id: &str, f: DalLavoratore, tx: &mpsc::UnboundedSender<String>) {
    let hub = s.motore.macchine();
    match f {
        DalLavoratore::Battito {
            posti_liberi,
            provider,
            lavori: in_corso,
        } => {
            tracing::debug!(macchina = %macchina_id, ?posti_liberi, lavori = in_corso.len(), "battito");
            hub.battito(macchina_id);
            if let Some(p) = posti_liberi {
                hub.mappe().posti_liberi.insert(macchina_id.to_string(), p);
            }
            let mid = macchina_id.to_string();
            let lease = ora_ms() + hub.tempi().lease_ms;
            if let Err(e) =
                s.db.esegui(move |c| {
                    macchine::battito_lavoratore(c, &mid, &provider)?;
                    lavori::rinnova_lease(c, &mid, lease)
                })
                .await
            {
                tracing::warn!(errore = %e, "battito del computer non scritto");
            }
        }
        DalLavoratore::Accesso { campi } => {
            s.accessi.dal_lavoratore(macchina_id, &Value::Object(campi));
        }
        DalLavoratore::Preso {
            lavoro_id,
            generazione,
            seq,
        } => {
            let dati = json!({ "generazione": generazione });
            let Some((nuovo, fino)) = salva(s, macchina_id, &lavoro_id, seq, "preso", dati).await
            else {
                return estraneo(tx, &lavoro_id);
            };
            if nuovo {
                hub.segna_preso(&lavoro_id);
                let lid = lavoro_id.clone();
                let lease = ora_ms() + hub.tempi().lease_ms;
                let _ =
                    s.db.esegui(move |c| {
                        lavori::segna(c, &lid, StatoLavoro::InCorso, None)?;
                        c.execute(
                            "UPDATE lavoro SET lease_scade_ms = ?2 WHERE id = ?1",
                            rusqlite::params![lid, lease],
                        )
                        .map_err(|e| Errore::database(e.to_string()))?;
                        Ok(())
                    })
                    .await;
            }
            conferma(tx, &lavoro_id, fino);
        }
        DalLavoratore::Evento {
            lavoro_id,
            seq,
            evento,
        } => {
            let Some((nuovo, fino)) =
                salva(s, macchina_id, &lavoro_id, seq, "evento", evento.clone()).await
            else {
                return estraneo(tx, &lavoro_id);
            };
            if nuovo {
                hub.segna_preso(&lavoro_id);
                match evento_da_filo(&evento) {
                    Some(ev) => {
                        if let Some(m) = hub.mittente(macchina_id, &lavoro_id) {
                            let _ = m.send(ev).await;
                        }
                    }
                    None => tracing::warn!(lavoro = %lavoro_id, "evento sconosciuto: saltato"),
                }
            }
            conferma(tx, &lavoro_id, fino);
        }
        DalLavoratore::Richiesta {
            lavoro_id,
            seq,
            richiesta_id,
            tipo_richiesta,
            carico,
        } => {
            let tipo = tipo_richiesta
                .or_else(|| carico["tipo"].as_str().map(str::to_string))
                .unwrap_or_else(|| "permesso_locale".into());
            let dati = json!({ "richiesta_id": richiesta_id, "tipo": tipo });
            let Some((nuovo, fino)) =
                salva(s, macchina_id, &lavoro_id, seq, "richiesta", dati).await
            else {
                return estraneo(tx, &lavoro_id);
            };
            if nuovo {
                inoltra_richiesta(s, macchina_id, &lavoro_id, richiesta_id, &tipo, carico).await;
            }
            conferma(tx, &lavoro_id, fino);
        }
        DalLavoratore::Chiuso {
            lavoro_id,
            seq,
            stato,
            motivo,
        } => {
            let dati = json!({ "stato": stato, "motivo": motivo });
            let Some((nuovo, fino)) = salva(s, macchina_id, &lavoro_id, seq, "chiuso", dati).await
            else {
                return estraneo(tx, &lavoro_id);
            };
            if nuovo {
                let finale = match stato.as_str() {
                    "riuscito" => StatoLavoro::Riuscito,
                    "interrotto" => StatoLavoro::Annullato,
                    _ => StatoLavoro::Fallito,
                };
                let (lid, esito) = (
                    lavoro_id.clone(),
                    json!({ "stato": stato, "motivo": motivo }),
                );
                let _ =
                    s.db.esegui(move |c| lavori::segna(c, &lid, finale, Some(&esito)))
                        .await;
                if let Some(v) = hub.togli(&lavoro_id)
                    && stato != "riuscito"
                {
                    let codice = if stato == "rifiutato" {
                        "lavoro_rifiutato".to_string()
                    } else {
                        format!("lavoro_{stato}")
                    };
                    let testo = motivo.unwrap_or_else(|| format!("il computer ha chiuso: {stato}"));
                    let _ = v.eventi.send(EventoTurno::Fallito { codice, testo }).await;
                }
            }
            conferma(tx, &lavoro_id, fino);
        }
    }
}

fn estraneo(tx: &mpsc::UnboundedSender<String>, lavoro_id: &str) {
    let _ = tx.send(
        json!({ "tipo": "errore", "codice": "lavoro_non_trovato", "lavoro_id": lavoro_id })
            .to_string(),
    );
}

/// Una richiesta di permesso del `claude` del computer: va nelle richieste dell'utente come quelle
/// del server; quando si chiude, il frame `risolta` torna al computer con il suo `richiesta_id`.
async fn inoltra_richiesta(
    s: &Stato,
    macchina_id: &str,
    lavoro_id: &str,
    richiesta_id: String,
    tipo: &str,
    carico: Value,
) {
    let lid = lavoro_id.to_string();
    let Ok(l) = s.db.esegui(move |c| lavori::leggi(c, &lid)).await else {
        return;
    };
    let sid = l["sessione_id"].as_str().unwrap_or("").to_string();
    let tipo = TipoRichiesta::da_testo(tipo).unwrap_or(TipoRichiesta::PermessoLocale);
    let mut carico = if carico.is_object() {
        carico
    } else {
        json!({ "descrizione": carico })
    };
    carico["turno_id"] = l["turno_id"].clone();
    carico["lavoro_id"] = json!(lavoro_id);
    carico["macchina_id"] = json!(macchina_id);
    let attesa = Duration::from_secs(15 * 60);
    let mid = macchina_id.to_string();
    let lid = lavoro_id.to_string();
    let rid = richiesta_id.clone();
    let risolta = move |esito: &str, comando: Option<String>, motivo: Option<String>| {
        json!({ "tipo": "risolta", "richiesta_id": richiesta_id, "esito": esito,
                "comando_approvato": comando, "motivo": motivo })
    };
    // 2.L-bis: prima i permessi dell'utente (collegamento, concessioni, regole), come sul server.
    if let Some((esito, motivo)) =
        permessi_mac::decidi_prima(s, &sid, l["turno_id"].as_str(), &carico).await
    {
        manda_risolta(s, &mid, &lid, &rid, risolta(&esito, None, motivo)).await;
        return;
    }
    match crate::richieste::crea(s, &sid, tipo, carico, Some(attesa)).await {
        Ok(r) => {
            let id = r["id"].as_str().unwrap_or_default().to_string();
            let s2 = s.clone();
            tokio::spawn(async move {
                let f = match crate::richieste::attendi(&s2, &id, attesa).await {
                    Ok(r) => risolta(r.esito.come_testo(), r.comando_approvato, None),
                    Err(e) => risolta("scaduta", None, Some(e.testo)),
                };
                manda_risolta(&s2, &mid, &lid, &rid, f).await;
            });
        }
        Err(e) => {
            let f = risolta("negata", None, Some(e.testo));
            manda_risolta(s, &mid, &lid, &rid, f).await;
        }
    }
}

/// Manda un frame `risolta`; se il computer è scollegato lo mette in coda
/// (`risolta_in_coda`) e parte al `ciao` della riconnessione.
async fn manda_risolta(
    s: &Stato,
    macchina_id: &str,
    lavoro_id: &str,
    richiesta_id: &str,
    f: Value,
) {
    let hub = s.motore.macchine();
    if hub.manda(macchina_id, &f) {
        return;
    }
    let (mid, lid, rid) = (
        macchina_id.to_string(),
        lavoro_id.to_string(),
        richiesta_id.to_string(),
    );
    match s
        .db
        .esegui(move |c| lavori::accoda_risolta(c, &mid, Some(&lid), &rid, &f))
        .await
    {
        Ok(()) => {
            tracing::info!(macchina = %macchina_id, "risposta alla richiesta in coda: computer scollegato")
        }
        Err(e) => {
            tracing::warn!(macchina = %macchina_id, errore = %e, "risposta alla richiesta persa")
        }
    }
    // Il computer può essersi ricollegato fra il tentativo e la coda.
    if hub.e_connessa(macchina_id) {
        consegna_risolte(s, macchina_id).await;
    }
}

/// Manda le `risolta` in coda di una macchina collegata (ognuna una volta sola: la presa e
/// la marcatura stanno nello stesso UPDATE). Il lavoratore le applica se il lavoro è vivo.
async fn consegna_risolte(s: &Stato, macchina_id: &str) {
    let mid = macchina_id.to_string();
    let frame = match s.db.esegui(move |c| lavori::prendi_risolte(c, &mid)).await {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(macchina = %macchina_id, errore = %e, "risposte in coda non lette");
            return;
        }
    };
    let hub = s.motore.macchine();
    for f in frame {
        if !hub.manda(macchina_id, &f) {
            // Caduto di nuovo: torna in coda.
            let (mid, rid) = (
                macchina_id.to_string(),
                f["richiesta_id"].as_str().unwrap_or_default().to_string(),
            );
            let _ = s
                .db
                .esegui(move |c| {
                    c.execute(
                        "UPDATE risolta_in_coda SET consegnata_ms = NULL WHERE richiesta_id = ?1 AND macchina_id = ?2",
                        rusqlite::params![rid, mid],
                    )
                    .map_err(|e| Errore::database(e.to_string()))?;
                    Ok(())
                })
                .await;
        }
    }
}

/// Il sorvegliante: macchine senza battito → `riconnessione`/`ferma`; lavori presi su una
/// macchina senza battito da più di `persa_ms` → falliti `macchina_persa`.
pub fn avvia_sorvegliante(s: &Stato) {
    let s = s.clone();
    tokio::spawn(async move {
        loop {
            let t = s.motore.macchine().tempi();
            tokio::time::sleep(Duration::from_millis(t.giro_ms.max(50))).await;
            giro(&s).await;
        }
    });
}

pub async fn giro(s: &Stato) {
    let hub = s.motore.macchine().clone();
    let t = hub.tempi();
    let ora = ora_ms();
    let (persi, senza_battito) = {
        let m = hub.mappe();
        let ultimo = |mid: &str| m.battiti.get(mid).copied().unwrap_or(0);
        let persi: Vec<String> = m
            .lavori
            .iter()
            .filter(|(_, v)| v.preso && ora - ultimo(&v.macchina_id) > t.persa_ms)
            .map(|(k, _)| k.clone())
            .collect();
        let senza: Vec<(String, bool)> = m
            .battiti
            .iter()
            .filter(|(_, b)| ora - **b > t.lease_ms)
            .map(|(mid, _)| {
                (
                    mid.clone(),
                    m.lavori.values().any(|v| v.macchina_id == *mid),
                )
            })
            .collect();
        (persi, senza)
    };
    for (mid, ha_lavori) in senza_battito {
        let stato = if ha_lavori { "riconnessione" } else { "ferma" };
        let _ =
            s.db.esegui(move |c| {
                c.execute(
                    "UPDATE macchina SET stato = ?2 WHERE id = ?1 AND stato = 'connessa'",
                    rusqlite::params![mid, stato],
                )
                .map_err(|e| Errore::database(e.to_string()))?;
                Ok(())
            })
            .await;
    }
    for lid in persi {
        let Some(v) = hub.togli(&lid) else { continue };
        tracing::warn!(lavoro = %lid, macchina = %v.macchina_id, "lavoro perso: computer senza battito");
        let (l2, m2) = (lid.clone(), v.macchina_id.clone());
        let _ =
            s.db.esegui(move |c| {
                lavori::segna(
                    c,
                    &l2,
                    StatoLavoro::Fallito,
                    Some(&json!({"motivo": "macchina_persa"})),
                )?;
                macchine::imposta_stato(c, &m2, "ferma")?;
                eventi::scrivi(
                    c,
                    "macchina_persa",
                    &json!({ "macchina_id": m2, "lavoro_id": l2, "motivo": "lease" }),
                )
            })
            .await;
        let _ = v
            .eventi
            .send(EventoTurno::Fallito {
                codice: "macchina_persa".into(),
                testo: "il computer non risponde da troppo tempo: turno perso".into(),
            })
            .await;
        let _ = (&v.sessione_id, &v.turno_id);
    }
}
