//! Sessioni figlie lato server: apertura (strumento `apri_figlia` e rotta dell'utente), stato nel
//! padre (la riga `evento` con `codice: figlia`, riscritta sul posto) e risultato che risale.
//! Le richieste della figlia compaiono nella riga del padre e si risolvono da entrambe le chat.

use std::future::Future;
use std::pin::Pin;

use serde_json::{Map, Value, json};
use xbot_core::{Risposta, nuovo_id};
use xbot_db::figlie::{self, Cambio};
use xbot_db::{Db, assistenti, sessioni};

use super::TurnoMcp;
use super::strumenti::Esito;
use crate::Stato;
use crate::bus::{Bus, Segnale};
use crate::motore::{DatiInvio, EsitoInvio, Motore, testo_del_turno};

fn testo<'a>(a: &'a Map<String, Value>, k: &str) -> &'a str {
    a.get(k).and_then(Value::as_str).unwrap_or_default().trim()
}

/// Ricalcola la figlia (se `figlia_id` lo è) e avvisa i client del padre. `con_testo` prende il
/// risultato dell'ultimo turno (alla fine di un turno).
pub async fn sincronizza(db: &Db, bus: &Bus, figlia_id: &str, con_testo: bool) -> Option<Cambio> {
    let sid = figlia_id.to_string();
    let r = db
        .esegui(move |c| {
            let t = if con_testo {
                let tid: Option<String> = c
                    .query_row(
                        "SELECT id FROM turno WHERE sessione_id = ?1 ORDER BY iniziato_ms DESC LIMIT 1",
                        [&sid],
                        |r| r.get(0),
                    )
                    .ok();
                tid.and_then(|t| testo_del_turno(c, &sid, &t).ok())
            } else {
                None
            };
            figlie::ricalcola(c, &sid, t.as_deref())
        })
        .await;
    match r {
        Ok(Some(k)) => {
            bus.pubblica(
                &k.padre_id,
                Segnale::Righe {
                    aggiornata_seq: k.seq_padre,
                },
            );
            bus.pubblica(&k.padre_id, Segnale::Barra);
            bus.pubblica(&k.figlia_id, Segnale::Barra);
            Some(k)
        }
        Ok(None) => None,
        Err(e) => {
            tracing::warn!(errore = %e, "stato della figlia non aggiornato");
            None
        }
    }
}

fn invia_in_scatola(
    m: Motore,
    sid: String,
    d: DatiInvio,
) -> Pin<Box<dyn Future<Output = Risposta<EsitoInvio>> + Send>> {
    Box::pin(async move { m.invia(&sid, d).await })
}

fn dati(testo: String, origine: String) -> DatiInvio {
    DatiInvio {
        id: nuovo_id(),
        testo,
        allegati: Value::Array(vec![]),
        risposta_a: None,
        provider: None,
        macchina: None,
        origine,
        composto_ms: None,
    }
}

/// Quando la figlia ha finito (o è fallita) il risultato risale al padre come invio `figlia:`.
pub async fn riporta(m: Motore, k: Cambio) {
    if !k.cambiato || !matches!(k.stato.as_str(), "finita" | "fallita") {
        return;
    }
    let titolo = k.titolo.clone().unwrap_or_else(|| "senza titolo".into());
    let esito = if k.stato == "finita" {
        "finita"
    } else {
        "fallita"
    };
    let corpo = k
        .riassunto
        .clone()
        .unwrap_or_else(|| "nessun risultato scritto".into());
    let t = format!("La chat figlia «{titolo}» è {esito}.\n\n{corpo}");
    if let Err(e) = invia_in_scatola(
        m,
        k.padre_id.clone(),
        dati(t, format!("figlia:{}", k.figlia_id)),
    )
    .await
    {
        tracing::warn!(errore = %e, "risultato della figlia non consegnato al padre");
    }
}

/// Apre una figlia di `padre_sid`. `menzione` vuota = lo stesso assistente del padre.
pub async fn apri_da(
    m: &Stato,
    padre_sid: &str,
    menzione: &str,
    compito: &str,
    titolo: &str,
) -> Result<Value, String> {
    if compito.is_empty() {
        return Err("serve il compito".into());
    }
    let titolo: String = if titolo.is_empty() {
        compito.chars().take(60).collect()
    } else {
        titolo.chars().take(80).collect()
    };
    let (padre, men) = (
        padre_sid.to_string(),
        menzione.trim_start_matches('@').to_lowercase(),
    );
    let (t2, p2) = (titolo.clone(), padre.clone());
    let (figlia, nome_padre) =
        m.db.esegui(move |c| {
            let sp = sessioni::leggi(c, &p2)?;
            let aid = if men.is_empty() {
                sp.assistente_id.clone()
            } else {
                assistenti::elenca(c, false)?
                    .iter()
                    .find(|a| {
                        a["menzione"].as_str().map(str::to_lowercase).as_deref() == Some(&men)
                    })
                    .and_then(|a| a["id"].as_str().map(str::to_string))
                    .ok_or_else(|| {
                        xbot_core::Errore::non_valida(format!("assistente @{men} non trovato"))
                    })?
            };
            let f = figlie::crea(c, &p2, &aid, Some(&t2))?;
            figlie::ricalcola(c, &f.id, None)?;
            let nome = assistenti::leggi(c, &sp.assistente_id)?["menzione"]
                .as_str()
                .unwrap_or("l'assistente")
                .to_string();
            Ok((f, nome))
        })
        .await
        .map_err(|e| e.testo)?;
    sincronizza(&m.db, &m.bus, &figlia.id, false).await;
    let t = format!(
        "Sei una chat figlia aperta da @{nome_padre} per un lavoro a parte. Compito:\n{compito}\n\n\
         Lavora in autonomia. Alla fine rispondi con il risultato in poche righe (al massimo 600 \
         caratteri): salirà alla chat principale. Se ti serve il permesso dell'utente, la richiesta \
         comparirà anche lì."
    );
    let e = invia_in_scatola(
        m.motore.clone(),
        figlia.id.clone(),
        dati(t, format!("figlia:{padre}")),
    )
    .await
    .map_err(|e| e.testo)?;
    Ok(json!({ "figlia_id": figlia.id, "titolo": titolo, "stato_invio": e.stato }))
}

/// `apri_figlia {compito, titolo?, menzione?}`.
pub async fn apri(m: &Stato, t: &TurnoMcp, a: &Map<String, Value>) -> Esito {
    let r = apri_da(
        m,
        &t.sessione_id,
        testo(a, "menzione"),
        testo(a, "compito"),
        testo(a, "titolo"),
    )
    .await?;
    Ok(format!(
        "Chat figlia aperta ({}). Ti scriverà il risultato quando ha finito: nel frattempo puoi continuare.",
        r["figlia_id"].as_str().unwrap_or("")
    ))
}
