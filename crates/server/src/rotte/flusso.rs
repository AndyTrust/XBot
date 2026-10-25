//! `GET /sessioni/{id}/flusso`: il flusso SSE di una sessione.
//!
//! Frame: `connected{stream_id, server_ms, vita_ms}`, `rows{generazione, righe[]}`,
//! `cleared{nuova_generazione}`, `cursor_too_old{generazione}`, `heartbeat{server_ms}`,
//! `turno{turno_id, stato}`, `chiusura{server_ms}` (il server si spegne: il flusso finisce).
//! Il bus dà solo il segnale; le righe si rileggono dal database.

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::response::sse::{Event, Sse};
use futures::Stream;
use serde_json::{Value, json};
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use xbot_core::{Codice, Risposta, nuovo_id, ora_ms};
use xbot_db::{righe, sessioni, turni};

use super::sessioni::Cursore;
use crate::bus::Segnale;
use crate::errore_http::{Domanda, ErroreHttp};
use crate::stato::Stato;

/// Vita massima di un flusso: poi il client si ricollega dal suo cursore.
pub const VITA: Duration = Duration::from_secs(30 * 60);
pub const BATTITO: Duration = Duration::from_secs(15);

type Uscita = mpsc::Sender<Result<Event, Infallible>>;

fn evento(nome: &str, dati: &Value) -> Result<Event, Infallible> {
    Ok(Event::default().event(nome).data(dati.to_string()))
}

pub async fn flusso(
    State(s): State<Stato>,
    Path(id): Path<String>,
    Domanda(q): Domanda<Cursore>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ErroreHttp> {
    let sid = id.clone();
    s.db.esegui(move |c| sessioni::leggi(c, &sid)).await?;
    // Prima ci si iscrive, poi si legge: nessun aggiornamento va perso.
    let bus = s.bus.iscrivi(&id);
    let (tx, rx) = mpsc::channel(32);
    tokio::spawn(async move {
        if let Err(e) = scorri(s, id, q, bus, tx).await {
            tracing::warn!(errore = %e, "flusso chiuso per errore");
        }
    });
    Ok(Sse::new(ReceiverStream::new(rx)))
}

/// Il cursore di un client dentro il flusso.
struct Posizione {
    generazione: Option<i64>,
    dopo: i64,
}

enum Lettura {
    Fatto,
    GenerazioneCambiata,
    ClientAndato,
}

/// Manda tutte le righe dopo il cursore, a pagine.
async fn manda_righe(s: &Stato, id: &str, pos: &mut Posizione, tx: &Uscita) -> Risposta<Lettura> {
    loop {
        let (sid, g, dopo) = (id.to_string(), pos.generazione, pos.dopo);
        let pagina = match s
            .db
            .esegui(move |c| righe::dopo(c, &sid, g, dopo, 500))
            .await
        {
            Ok(p) => p,
            Err(e) if e.codice == Codice::CursoreVecchio => {
                return Ok(Lettura::GenerazioneCambiata);
            }
            Err(e) => return Err(e),
        };
        pos.generazione = Some(pagina.generazione);
        if let Some(ultima) = pagina.righe.last() {
            pos.dopo = ultima.aggiornata_seq;
            let dati = json!({ "generazione": pagina.generazione, "righe": pagina.righe });
            if tx.send(evento("rows", &dati)).await.is_err() {
                return Ok(Lettura::ClientAndato);
            }
        }
        if !pagina.altre {
            return Ok(Lettura::Fatto);
        }
    }
}

async fn scorri(
    s: Stato,
    id: String,
    q: Cursore,
    mut bus: broadcast::Receiver<Segnale>,
    tx: Uscita,
) -> Risposta<()> {
    // Ci si iscrive alla chiusura prima di tutto: una chiusura durante l'avvio non va persa.
    let mut chiusura = s.chiusura.subscribe();
    let connesso = json!({
        "stream_id": nuovo_id(),
        "server_ms": ora_ms(),
        "vita_ms": i64::try_from(VITA.as_millis()).unwrap_or(i64::MAX),
    });
    if tx.send(evento("connected", &connesso)).await.is_err() {
        return Ok(());
    }
    let mut pos = Posizione {
        generazione: q.generazione,
        dopo: q.dopo_aggiornata_seq.unwrap_or(0),
    };
    match manda_righe(&s, &id, &mut pos, &tx).await? {
        Lettura::Fatto => {}
        Lettura::ClientAndato => return Ok(()),
        Lettura::GenerazioneCambiata => {
            let sid = id.clone();
            let g =
                s.db.esegui(move |c| sessioni::leggi(c, &sid))
                    .await?
                    .generazione;
            let _ = tx
                .send(evento("cursor_too_old", &json!({ "generazione": g })))
                .await;
            return Ok(());
        }
    }
    let sid = id.clone();
    if let Some(t) =
        s.db.esegui(move |c| turni::ultimo_della_sessione(c, &sid))
            .await?
        && t.stato == "in_corso"
    {
        let dati = json!({ "turno_id": t.id, "stato": t.stato });
        if tx.send(evento("turno", &dati)).await.is_err() {
            return Ok(());
        }
    }
    if *chiusura.borrow_and_update() {
        let _ = tx
            .send(evento("chiusura", &json!({ "server_ms": ora_ms() })))
            .await;
        return Ok(());
    }
    let scadenza = tokio::time::sleep(VITA);
    tokio::pin!(scadenza);
    let mut battito = tokio::time::interval(BATTITO);
    battito.tick().await;
    loop {
        let lettura = tokio::select! {
            () = &mut scadenza => return Ok(()),
            () = tx.closed() => return Ok(()),
            // Il valore passa solo da false a true: ogni cambio è la chiusura.
            _ = chiusura.changed() => {
                let _ = tx.send(evento("chiusura", &json!({ "server_ms": ora_ms() }))).await;
                return Ok(());
            }
            _ = battito.tick() => {
                if tx.send(evento("heartbeat", &json!({ "server_ms": ora_ms() }))).await.is_err() {
                    return Ok(());
                }
                continue;
            }
            segnale = bus.recv() => match segnale {
                Ok(Segnale::Righe { .. }) | Err(broadcast::error::RecvError::Lagged(_)) => {
                    manda_righe(&s, &id, &mut pos, &tx).await?
                }
                Ok(Segnale::Azzerata { .. }) => Lettura::GenerazioneCambiata,
                Ok(Segnale::Turno { turno_id, stato }) => {
                    let dati = json!({ "turno_id": turno_id, "stato": stato });
                    if tx.send(evento("turno", &dati)).await.is_err() {
                        return Ok(());
                    }
                    // L'inizio e la fine di un turno cambiano «al lavoro» e «tocca a te».
                    if tx.send(evento("barra", &json!({}))).await.is_err() {
                        return Ok(());
                    }
                    continue;
                }
                Ok(Segnale::Richiesta { id, tipo, stato }) => {
                    let dati = json!({ "id": id, "tipo": tipo, "stato": stato });
                    if tx.send(evento("richiesta", &dati)).await.is_err() {
                        return Ok(());
                    }
                    continue;
                }
                Ok(Segnale::Barra) => {
                    if tx.send(evento("barra", &json!({}))).await.is_err() {
                        return Ok(());
                    }
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => return Ok(()),
            },
        };
        match lettura {
            Lettura::Fatto => {}
            Lettura::ClientAndato => return Ok(()),
            Lettura::GenerazioneCambiata => {
                let sid = id.clone();
                let g =
                    s.db.esegui(move |c| sessioni::leggi(c, &sid))
                        .await?
                        .generazione;
                if tx
                    .send(evento("cleared", &json!({ "nuova_generazione": g })))
                    .await
                    .is_err()
                {
                    return Ok(());
                }
                pos = Posizione {
                    generazione: Some(g),
                    dopo: 0,
                };
                if let Lettura::ClientAndato = manda_righe(&s, &id, &mut pos, &tx).await? {
                    return Ok(());
                }
            }
        }
    }
}
