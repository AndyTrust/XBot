//! `xbot-worker run`: il canale WebSocket col server, sempre acceso.
//!
//! Un solo compito tiene il socket, il giornale e i lavori attivi: niente corse fra chi scrive
//! e chi rimanda. I lavori girano in compiti loro e mandano i frame per l'uscita; se il socket
//! è giù i frame restano nel giornale e partono alla riconnessione (la chiusura del socket non
//! annulla il turno). Battito ogni `conf.battito` (20 s).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use xbot_core::{Errore, Risposta};

use crate::credenziale::Credenziale;
use crate::esecuzione::{self, Lavoro};
use crate::giornale::Giornale;
use crate::permessi::{self, Permessi, Risoluzione};
use crate::protocollo::{Corpo, DalLavoratore, DalServer, ProviderMacchina};
use crate::{Config, scoperta};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Ogni quanto si riscoprono i client del Mac.
const RISCOPERTA: Duration = Duration::from_secs(600);
const ATTESA_MAX: Duration = Duration::from_secs(30);
/// 2.I: ogni quanto si rileggono gli account dei client.
const CONTI: Duration = Duration::from_secs(120);

struct Attivo {
    sessione_id: String,
    generazione: i64,
    stop: Option<oneshot::Sender<()>>,
}

struct Lavoratore {
    conf: Arc<Config>,
    cred: Credenziale,
    giornale: Giornale,
    attivi: HashMap<String, Attivo>,
    /// La generazione più alta vista per sessione (il recinto).
    generazioni: HashMap<String, i64>,
    permessi: Arc<Permessi>,
    uscita: mpsc::Sender<(String, Corpo)>,
    provider: Vec<ProviderMacchina>,
    /// 2.I: «Entra» ed «Esci» dei client.
    accessi: crate::accessi::Accessi,
}

/// Il frame `lavoro` può portare 25 MB di allegati (33 MB in base64): limiti più larghi dei
/// 16 MB predefiniti per frame.
fn config_ws() -> tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
    tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(64 << 20))
        .max_frame_size(Some(64 << 20))
}

/// Connette con `Authorization: Bearer <credenziale>`.
async fn connetti(cred: &Credenziale) -> Risposta<Ws> {
    let url = cred.url_canale();
    let mut req = url
        .as_str()
        .into_client_request()
        .map_err(|e| Errore::non_valida(format!("indirizzo del canale {url}: {e}")))?;
    let valore = format!("Bearer {}", cred.credenziale)
        .parse()
        .map_err(|_| Errore::non_valida("credenziale non valida come intestazione"))?;
    req.headers_mut().insert("authorization", valore);
    let (ws, _) = tokio::time::timeout(
        Duration::from_secs(15),
        tokio_tungstenite::connect_async_with_config(req, Some(config_ws()), false),
    )
    .await
    .map_err(|_| Errore::interno(format!("{url}: nessuna risposta in 15 s")))?
    .map_err(|e| Errore::interno(format!("{url}: {e}")))?;
    Ok(ws)
}

async fn prossimo(ws: &mut Option<Ws>) -> Option<Result<Message, String>> {
    match ws {
        Some(w) => w.next().await.map(|r| r.map_err(|e| e.to_string())),
        None => std::future::pending().await,
    }
}

impl Lavoratore {
    /// I posti liberi: contano i lavori che lavorano davvero. Un processo caldo a riposo non è
    /// un lavoro (esce da `attivi` a fine turno); un lavoro fermo su un permesso dell'utente non
    /// occupa posto (2.L-bis).
    fn posti_liberi(&self) -> u32 {
        let fermi = self.permessi.lavori_in_attesa();
        let occupati = self.attivi.keys().filter(|k| !fermi.contains(*k)).count();
        self.conf
            .posti
            .saturating_sub(u32::try_from(occupati).unwrap_or(u32::MAX))
    }

    fn battito(&self) -> String {
        let f = DalLavoratore::Battito {
            posti_liberi: self.posti_liberi(),
            provider: self.provider.clone(),
            lavori: self.attivi.keys().cloned().collect(),
        };
        serde_json::to_string(&f).unwrap_or_default()
    }

    /// Scrive nel giornale e restituisce la riga da spedire.
    fn registra(&mut self, lavoro_id: &str, corpo: Corpo) -> Option<String> {
        if let Corpo::Chiuso { .. } = corpo {
            self.attivi.remove(lavoro_id);
        }
        match self.giornale.aggiungi(lavoro_id, corpo) {
            Ok(r) => Some(r),
            Err(e) => {
                tracing::warn!(lavoro = %lavoro_id, errore = %e, "frame scartato");
                None
            }
        }
    }

    /// Un frame dal server; restituisce le righe da spedire.
    fn dal_server(&mut self, f: DalServer) -> Vec<String> {
        match f {
            DalServer::Ciao {
                macchina_id,
                server_versione,
            } => {
                tracing::info!(macchina = %macchina_id, server = ?server_versione, "canale aperto");
                Vec::new()
            }
            DalServer::Ok {
                lavoro_id,
                fino_a_seq,
            } => {
                self.giornale.conferma(&lavoro_id, fino_a_seq);
                Vec::new()
            }
            // --- 2.I ---
            DalServer::Accedi {
                richiesta_id,
                provider_id,
            } => {
                let p = esecuzione::adattatore(&self.conf, &provider_id, false);
                self.accessi.accedi(richiesta_id, provider_id, p);
                Vec::new()
            }
            DalServer::Esci {
                richiesta_id,
                provider_id,
            } => {
                let p = esecuzione::adattatore(&self.conf, &provider_id, false);
                self.accessi.esci(richiesta_id, provider_id, p);
                Vec::new()
            }
            DalServer::ProvaCollegamento {
                richiesta_id,
                collegamento,
            } => {
                crate::prova::avvia(self.accessi.grezzi.clone(), richiesta_id, *collegamento);
                Vec::new()
            }
            DalServer::CodiceAccesso {
                provider_id,
                codice,
            } => {
                self.accessi.codice(provider_id, codice);
                Vec::new()
            }
            DalServer::Interrompi { lavoro_id } => {
                if let Some(a) = self.attivi.get_mut(&lavoro_id)
                    && let Some(s) = a.stop.take()
                {
                    let _ = s.send(());
                }
                Vec::new()
            }
            DalServer::Risolta {
                richiesta_id,
                esito,
                comando_approvato,
                motivo,
            } => {
                let r = Risoluzione {
                    esito,
                    comando_approvato,
                    motivo,
                };
                if !self.permessi.risolvi(&richiesta_id, r) {
                    tracing::info!(richiesta = %richiesta_id, "risolta senza nessuno in attesa");
                }
                Vec::new()
            }
            DalServer::Lavoro {
                id,
                generazione,
                sessione_id,
                turno_id,
                mut richiesta,
                chrome,
                ..
            } => {
                richiesta.chrome |= chrome;
                let l = Lavoro {
                    id,
                    generazione,
                    sessione_id,
                    turno_id,
                    richiesta: *richiesta,
                };
                self.nuovo_lavoro(l)
            }
            DalServer::Sconosciuto => Vec::new(),
        }
    }

    fn rifiuta(&mut self, id: &str, motivo: &str) -> Vec<String> {
        tracing::info!(lavoro = %id, motivo, "lavoro rifiutato");
        self.registra(
            id,
            Corpo::Chiuso {
                stato: "rifiutato".into(),
                motivo: Some(motivo.into()),
            },
        )
        .into_iter()
        .collect()
    }

    fn nuovo_lavoro(&mut self, l: Lavoro) -> Vec<String> {
        // Lo stesso lavoro rimandato dal server dopo una riconnessione: è già qui.
        if self.attivi.contains_key(&l.id) || self.giornale.conosce(&l.id) {
            return Vec::new();
        }
        let vista = self.generazioni.get(&l.sessione_id).copied();
        if vista.is_some_and(|g| l.generazione < g) {
            return self.rifiuta(&l.id, "generazione vecchia per questa sessione");
        }
        let mut righe = Vec::new();
        // Un solo lavoro per sessione: il più nuovo prende il posto del vecchio.
        let vecchi: Vec<String> = self
            .attivi
            .iter()
            .filter(|(_, a)| a.sessione_id == l.sessione_id && a.generazione <= l.generazione)
            .map(|(k, _)| k.clone())
            .collect();
        for k in vecchi {
            if let Some(a) = self.attivi.get_mut(&k)
                && let Some(s) = a.stop.take()
            {
                let _ = s.send(());
            }
        }
        if self.posti_liberi() == 0 && !self.attivi.values().any(|a| a.sessione_id == l.sessione_id)
        {
            return self.rifiuta(&l.id, "nessun posto libero sul Mac");
        }
        self.generazioni
            .insert(l.sessione_id.clone(), l.generazione);
        righe.extend(self.registra(
            &l.id,
            Corpo::Preso {
                generazione: l.generazione,
            },
        ));
        tracing::info!(lavoro = %l.id, provider = %l.richiesta.provider_id, chrome = l.richiesta.chrome, "lavoro preso");
        let stop = esecuzione::avvia(
            self.conf.clone(),
            l.clone(),
            self.uscita.clone(),
            self.permessi.clone(),
        );
        self.attivi.insert(
            l.id.clone(),
            Attivo {
                sessione_id: l.sessione_id,
                generazione: l.generazione,
                stop: Some(stop),
            },
        );
        righe
    }
}

/// Spedisce le righe; falso se il socket è caduto.
async fn spedisci(ws: &mut Option<Ws>, righe: Vec<String>) {
    let Some(w) = ws.as_mut() else { return };
    for r in righe {
        if let Err(e) = w.send(Message::text(r)).await {
            tracing::warn!(errore = %e, "socket caduto in scrittura");
            *ws = None;
            return;
        }
    }
}

/// Il collegamento `bin/claude-mcp-utente` → questo eseguibile (per i turni con Chrome).
fn prepara_involucro(conf: &Config) {
    let bin = conf.cartella.join("bin");
    let link = bin.join(esecuzione::NOME_CLAUDE_UTENTE);
    let _ = std::fs::create_dir_all(&bin);
    let _ = std::fs::remove_file(&link);
    if let Err(e) = std::os::unix::fs::symlink(&conf.programma, &link) {
        tracing::warn!(errore = %e, "involucro per Chrome non creato");
    }
}

/// Il ciclo del lavoratore, finché `fermo` non diventa vero.
pub async fn esegui(
    conf: Config,
    cred: Credenziale,
    mut fermo: watch::Receiver<bool>,
) -> Risposta<()> {
    let conf = Arc::new(conf);
    prepara_involucro(&conf);
    let (uscita, mut rx) = mpsc::channel::<(String, Corpo)>(256);
    let permessi = Arc::new(Permessi::nuovo(&conf.cartella, uscita.clone()));
    permessi::avvia(permessi.clone()).await?;
    let mut giornale = Giornale::apri(&conf.cartella)?;
    // Lavori rimasti aperti da un'esecuzione precedente: il processo del client non c'è più.
    for id in giornale.aperti() {
        giornale.aggiungi(
            &id,
            Corpo::Chiuso {
                stato: "fallito".into(),
                motivo: Some("lavoratore riavviato durante il turno".into()),
            },
        )?;
    }
    let (grezzi_tx, mut grezzi_rx) = mpsc::unbounded_channel::<String>();
    let (cambi_tx, mut cambi_rx) = mpsc::unbounded_channel::<crate::accessi::Cambio>();
    let provider = match &conf.provider {
        Some(p) => p.clone(),
        None => scoperta::scopri().await,
    };
    tracing::info!(client = provider.len(), "client del Mac trovati");
    let mut w = Lavoratore {
        conf: conf.clone(),
        cred,
        giornale,
        attivi: HashMap::new(),
        generazioni: HashMap::new(),
        permessi: permessi.clone(),
        uscita,
        provider,
        accessi: crate::accessi::Accessi::nuovo(grezzi_tx, cambi_tx),
    };
    let mut ws: Option<Ws> = None;
    let mut attesa = Duration::from_millis(500);
    let mut prossimo_tentativo = Instant::now();
    let mut battito = tokio::time::interval(conf.battito);
    let mut riscoperta = tokio::time::interval(RISCOPERTA);
    riscoperta.tick().await;
    let (scoperti_tx, mut scoperti_rx) = mpsc::channel::<Vec<ProviderMacchina>>(1);
    let mut conti = tokio::time::interval(CONTI);
    conti.tick().await;
    loop {
        if *fermo.borrow() {
            break;
        }
        if ws.is_none() && Instant::now() >= prossimo_tentativo {
            match connetti(&w.cred).await {
                Ok(nuovo) => {
                    tracing::info!("collegato al server");
                    ws = Some(nuovo);
                    attesa = Duration::from_millis(500);
                    let mut righe = vec![w.battito()];
                    righe.extend(w.giornale.da_rimandare());
                    spedisci(&mut ws, righe).await;
                }
                Err(e) => {
                    tracing::warn!(errore = %e, attesa_s = attesa.as_secs_f32(), "server non raggiungibile");
                    prossimo_tentativo = Instant::now() + attesa;
                    attesa = (attesa * 2).min(ATTESA_MAX);
                }
            }
            continue;
        }
        tokio::select! {
            r = fermo.changed() => if r.is_err() { break },
            m = rx.recv() => {
                let Some((id, corpo)) = m else { break };
                let righe: Vec<String> = w.registra(&id, corpo).into_iter().collect();
                spedisci(&mut ws, righe).await;
            }
            f = prossimo(&mut ws) => match f {
                Some(Ok(Message::Text(t))) => match serde_json::from_str::<DalServer>(t.as_str()) {
                    Ok(f) => {
                        let righe = w.dal_server(f);
                        spedisci(&mut ws, righe).await;
                    }
                    Err(e) => tracing::warn!(errore = %e, "frame del server illeggibile"),
                },
                Some(Ok(Message::Close(_))) | None => {
                    tracing::warn!("il server ha chiuso il canale");
                    ws = None;
                    prossimo_tentativo = Instant::now() + attesa;
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => {
                    tracing::warn!(errore = %e, "canale caduto");
                    ws = None;
                    prossimo_tentativo = Instant::now() + attesa;
                }
            },
            _ = battito.tick() => {
                let b = w.battito();
                spedisci(&mut ws, vec![b]).await;
            }
            _ = riscoperta.tick(), if conf.provider.is_none() => {
                let tx = scoperti_tx.clone();
                tokio::spawn(async move { let _ = tx.send(scoperta::scopri().await).await; });
            }
            Some(p) = scoperti_rx.recv() => { w.provider = p; }
            // --- 2.I --- frame `accesso` e account cambiati (subito un battito nuovo).
            Some(r) = grezzi_rx.recv() => spedisci(&mut ws, vec![r]).await,
            Some((id, account)) = cambi_rx.recv() => {
                if let Some(p) = w.provider.iter_mut().find(|p| p.id == id) {
                    p.account = account;
                }
                let b = w.battito();
                spedisci(&mut ws, vec![b]).await;
            }
            _ = conti.tick(), if conf.provider.is_none() => {
                let (tx, conf2) = (w.accessi.cambi.clone(), w.conf.clone());
                let ids: Vec<String> = w.provider.iter().map(|p| p.id.clone()).collect();
                tokio::spawn(async move {
                    for id in ids {
                        if let Ok(p) = esecuzione::adattatore(&conf2, &id, false) {
                            let _ = tx.send((id, scoperta::leggi_account(p.as_ref()).await));
                        }
                    }
                });
            }
            () = tokio::time::sleep_until(prossimo_tentativo), if ws.is_none() => {}
        }
    }
    for a in w.attivi.values_mut() {
        if let Some(s) = a.stop.take() {
            let _ = s.send(());
        }
    }
    if let Some(mut s) = ws {
        let _ = s.close(None).await;
    }
    permessi_togli_socket(&permessi);
    Ok(())
}

fn permessi_togli_socket(p: &Permessi) {
    let _ = std::fs::remove_file(&p.socket);
}
