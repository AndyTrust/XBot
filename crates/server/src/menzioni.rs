//! Menzioni (Fase 2.D S2): gli assistenti si parlano con `@nome`.
//!
//! - In un invio dell'utente, `@nome` manda il messaggio anche nella sessione corrente
//!   dell'assistente nominato, incorniciato «L'utente, dalla chat con A, ti dice: …»
//!   (origine `menzione:<sessione>`).
//! - Nel testo di un assistente (a fine turno) o con lo strumento `invia_a`, `@nome` crea un
//!   invio `collega` «A ti scrive: …» (origine `collega:<sessione>:<passo>`): il testo di un pari
//!   non è mai un ordine. La risposta torna solo se B scrive a sua volta `@A`.
//! - Protezioni: 5 invii fra assistenti ogni 10 minuti per coppia, catene di al massimo 3 passi
//!   senza un invio dell'utente in mezzo; oltre, riga `avviso` «Catena fra assistenti fermata» e
//!   «tocca a te» sull'assistente che scriveva. `@tutti` non esiste; un `@nome` sconosciuto resta testo.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use serde_json::{Value, json};
use xbot_core::{GenereRiga, Risposta, Ruolo, nuovo_id, ora_ms};
use xbot_db::sessioni::{self, NuovaSessione};
use xbot_db::{Connessione, assistenti, lettura, righe, tutte_json};

use crate::bus::Segnale;
use crate::mcp_interno::TurnoMcp;
use crate::mcp_interno::strumenti::Esito;
use crate::motore::{DatiInvio, EsitoInvio, Motore};

pub const MAX_PER_COPPIA: usize = 5;
pub const FINESTRA_MS: i64 = 10 * 60 * 1000;
pub const MAX_PASSI: u32 = 3;
pub const AVVISO_CATENA: &str = "Catena fra assistenti fermata";

/// Le menzioni in un testo, in ordine e senza ripetizioni. `@` deve stare a inizio testo o dopo
/// un carattere che non è lettera né cifra (così un indirizzo email non conta).
pub fn trova(testo: &str) -> Vec<String> {
    let mut fuori: Vec<String> = Vec::new();
    let c: Vec<char> = testo.chars().collect();
    let mut i = 0;
    while i < c.len() {
        let prima_ok = i == 0 || !c[i - 1].is_alphanumeric();
        if c[i] == '@' && prima_ok {
            let mut j = i + 1;
            while j < c.len() && (c[j].is_alphanumeric() || matches!(c[j], '_' | '-' | '.')) {
                j += 1;
            }
            let nome: String = c[i + 1..j].iter().collect::<String>();
            let nome = nome.trim_end_matches(['.', '-']).to_lowercase();
            if !nome.is_empty() && nome != "tutti" && !fuori.contains(&nome) {
                fuori.push(nome);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    fuori
}

/// Il passo della catena di un invio: `collega:<sessione>:<passo>`, altrimenti 0.
pub fn passo_da_origine(origine: &str) -> u32 {
    origine
        .strip_prefix("collega:")
        .and_then(|r| r.rsplit(':').next())
        .and_then(|p| p.parse().ok())
        .unwrap_or(0)
}

/// Le origini che non vengono dall'utente: le loro menzioni non si inoltrano come «dell'utente».
pub fn origine_dell_utente(origine: &str) -> bool {
    !["menzione:", "collega:", "canale:", "routine:", "figlia:"]
        .iter()
        .any(|p| origine.starts_with(p))
}

/// Il conto degli invii fra assistenti per coppia (in memoria).
#[derive(Default)]
pub struct Limiti {
    coppie: Mutex<HashMap<(String, String), VecDeque<i64>>>,
}

impl Limiti {
    /// Vero (e conta l'invio) se la coppia è sotto il limite nella finestra.
    pub fn consenti(&self, da: &str, a: &str, ora: i64) -> bool {
        let mut g = match self.coppie.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        let coda = g.entry((da.to_string(), a.to_string())).or_default();
        while coda.front().is_some_and(|t| ora - t >= FINESTRA_MS) {
            coda.pop_front();
        }
        if coda.len() >= MAX_PER_COPPIA {
            return false;
        }
        coda.push_back(ora);
        true
    }
}

/// Chi scrive.
#[derive(Debug, Clone)]
pub enum Mittente {
    Utente {
        sessione_id: String,
        assistente_id: String,
    },
    Assistente {
        sessione_id: String,
        assistente_id: String,
        passo: u32,
    },
}

fn invia_in_scatola(
    m: Motore,
    sid: String,
    d: DatiInvio,
) -> Pin<Box<dyn Future<Output = Risposta<EsitoInvio>> + Send>> {
    Box::pin(async move { m.invia(&sid, d).await })
}

/// La sessione corrente di un assistente: quella segnata, o l'ultima attiva, o una nuova.
pub fn sessione_corrente(c: &Connessione, assistente_id: &str) -> Risposta<String> {
    let segnata = tutte_json(
        c,
        "SELECT s.id FROM stato_lettura l JOIN sessione s ON s.id = l.sessione_corrente_id
         WHERE l.assistente_id = ?1 AND s.stato = 'attiva'",
        [assistente_id],
    )?;
    let ultima = || {
        tutte_json(
            c,
            "SELECT id FROM sessione WHERE assistente_id = ?1 AND stato = 'attiva'
               AND sessione_padre_id IS NULL ORDER BY aggiornata_ms DESC LIMIT 1",
            [assistente_id],
        )
    };
    let trovata = match segnata.first() {
        Some(v) => Some(v.clone()),
        None => ultima()?.first().cloned(),
    };
    match trovata.and_then(|v| v["id"].as_str().map(str::to_string)) {
        Some(id) => Ok(id),
        None => Ok(sessioni::crea(c, assistente_id, &NuovaSessione::default())?.id),
    }
}

/// Riga `avviso` «Catena fra assistenti fermata» e «tocca a te» su chi scriveva.
async fn ferma(m: &Motore, sessione_id: &str, assistente_id: &str, motivo: &str) {
    let corpo =
        json!({ "codice": "catena_fermata", "testo": format!("{AVVISO_CATENA}: {motivo}") });
    let (sid, aid) = (sessione_id.to_string(), assistente_id.to_string());
    let r = m
        .db()
        .esegui(move |c| {
            let r = righe::aggiungi(c, &sid, GenereRiga::Avviso, Some(Ruolo::Sistema), &corpo)?;
            lettura::imposta_lavoro(c, &aid, false, true)?;
            Ok(r)
        })
        .await;
    match r {
        Ok(r) => m.bus().pubblica(
            sessione_id,
            Segnale::Righe {
                aggiornata_seq: r.aggiornata_seq,
            },
        ),
        Err(e) => tracing::error!(errore = %e, "avviso della catena non scritto"),
    }
    m.bus().pubblica(sessione_id, Segnale::Barra);
    tracing::info!(sessione = %sessione_id, motivo, "catena fra assistenti fermata");
}

/// Consegna un messaggio all'assistente `@menzione`. `Err` = non consegnato (con il motivo).
pub async fn consegna(m: &Motore, da: &Mittente, menzione: &str, testo: &str) -> Esito {
    let menzione = menzione.trim().trim_start_matches('@').to_lowercase();
    let elenco = m
        .db()
        .esegui(|c| assistenti::elenca(c, false))
        .await
        .map_err(|e| e.testo)?;
    let (sid_da, aid_da) = match da {
        Mittente::Utente {
            sessione_id,
            assistente_id,
        }
        | Mittente::Assistente {
            sessione_id,
            assistente_id,
            ..
        } => (sessione_id.clone(), assistente_id.clone()),
    };
    // `menzione` la aggiunge `assistenti::elenca` (S1, `assistenti::menzioni`).
    let Some(destinatario) = elenco
        .iter()
        .find(|a| a["menzione"].as_str() == Some(menzione.as_str()))
    else {
        return Err(format!("nessun assistente @{menzione}"));
    };
    let aid_a = destinatario["id"].as_str().unwrap_or_default().to_string();
    if aid_a == aid_da {
        return Err("un assistente non scrive a sé stesso".into());
    }
    let nome_da = elenco
        .iter()
        .find(|a| a["id"].as_str() == Some(aid_da.as_str()))
        .and_then(|a| a["nome"].as_str())
        .unwrap_or("un assistente")
        .to_string();
    let (cornice, origine) = match da {
        Mittente::Utente { .. } => (
            format!(
                "{}, dalla chat con {nome_da}, ti dice: {testo}",
                m.db()
                    .esegui(|c| Ok(xbot_db::nome_utente(c)))
                    .await
                    .unwrap_or_else(|_| "L'utente".into())
            ),
            format!("menzione:{sid_da}"),
        ),
        Mittente::Assistente { passo, .. } => {
            if *passo > MAX_PASSI {
                ferma(
                    m,
                    &sid_da,
                    &aid_da,
                    &format!("più di {MAX_PASSI} passi senza l'utente"),
                )
                .await;
                return Err(format!(
                    "{AVVISO_CATENA}: più di {MAX_PASSI} passi senza un messaggio dell'utente"
                ));
            }
            if !m.s2().menzioni.consenti(&aid_da, &aid_a, ora_ms()) {
                ferma(
                    m,
                    &sid_da,
                    &aid_da,
                    &format!("più di {MAX_PER_COPPIA} messaggi in 10 minuti"),
                )
                .await;
                return Err(format!(
                    "{AVVISO_CATENA}: più di {MAX_PER_COPPIA} messaggi in 10 minuti a @{menzione}"
                ));
            }
            (
                format!("{nome_da} ti scrive: {testo}"),
                format!("collega:{sid_da}:{passo}"),
            )
        }
    };
    let aid2 = aid_a.clone();
    let sid_a = m
        .db()
        .esegui(move |c| sessione_corrente(c, &aid2))
        .await
        .map_err(|e| e.testo)?;
    let d = DatiInvio {
        id: nuovo_id(),
        testo: cornice,
        allegati: Value::Array(vec![]),
        risposta_a: None,
        provider: None,
        macchina: None,
        origine,
        composto_ms: None,
    };
    let e = invia_in_scatola(m.clone(), sid_a.clone(), d)
        .await
        .map_err(|e| e.testo)?;
    tracing::info!(da = %aid_da, a = %aid_a, stato = %e.stato, "messaggio fra assistenti");
    Ok(format!(
        "Consegnato a @{menzione} (sessione {sid_a}, invio {}, {}).",
        e.invio_id, e.stato
    ))
}

/// Le menzioni in un invio dell'utente: ognuna diventa un invio nell'assistente nominato.
pub async fn da_utente(m: &Motore, sessione_id: &str, testo: &str) {
    let trovate = trova(testo);
    if trovate.is_empty() {
        return;
    }
    let sid = sessione_id.to_string();
    let Ok(s) = m.db().esegui(move |c| sessioni::leggi(c, &sid)).await else {
        return;
    };
    let da = Mittente::Utente {
        sessione_id: sessione_id.into(),
        assistente_id: s.assistente_id,
    };
    for n in trovate {
        if let Err(e) = consegna(m, &da, &n, testo).await {
            tracing::debug!(menzione = %n, motivo = %e, "menzione dell'utente non inoltrata");
        }
    }
}

/// Le menzioni nel testo di un assistente a fine turno.
pub async fn da_assistente(
    m: &Motore,
    sessione_id: &str,
    assistente_id: &str,
    passo_invio: u32,
    testo: &str,
) {
    let da = Mittente::Assistente {
        sessione_id: sessione_id.into(),
        assistente_id: assistente_id.into(),
        passo: passo_invio + 1,
    };
    for n in trova(testo) {
        if let Err(e) = consegna(m, &da, &n, testo).await {
            tracing::debug!(menzione = %n, motivo = %e, "menzione dell'assistente non inoltrata");
        }
    }
}

/// Lo strumento `invia_a {menzione, testo}` del server interno.
pub async fn da_strumento(m: &Motore, t: &TurnoMcp, menzione: &str, testo: &str) -> Esito {
    if testo.trim().is_empty() || menzione.trim().is_empty() {
        return Err("servono menzione e testo".into());
    }
    let da = Mittente::Assistente {
        sessione_id: t.sessione_id.clone(),
        assistente_id: t.assistente_id.clone(),
        passo: t.passo + 1,
    };
    consegna(m, &da, menzione, testo).await
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn menzioni_nel_testo() {
        assert_eq!(trova("@Atena e @ceo-ai, guardate"), vec!["atena", "ceo-ai"]);
        assert_eq!(trova("scrivi a utente@esempio.it"), Vec::<String>::new());
        assert_eq!(trova("@tutti @atena. @atena"), vec!["atena"]);
        assert_eq!(trova("(@Ricercatore_web)"), vec!["ricercatore_web"]);
    }

    #[test]
    fn passi_e_origini() {
        assert_eq!(passo_da_origine("collega:S1:2"), 2);
        assert_eq!(passo_da_origine("menzione:S1"), 0);
        assert_eq!(passo_da_origine("pwa"), 0);
        assert!(origine_dell_utente("pwa") && origine_dell_utente("voce"));
        assert!(!origine_dell_utente("collega:S:1") && !origine_dell_utente("canale:x"));
    }

    #[test]
    fn limite_per_coppia() {
        let l = Limiti::default();
        for i in 0..5 {
            assert!(l.consenti("a", "b", i));
        }
        assert!(!l.consenti("a", "b", 10));
        assert!(
            l.consenti("b", "a", 10),
            "l'altra direzione ha il suo conto"
        );
        assert!(
            l.consenti("a", "b", FINESTRA_MS + 1),
            "fuori finestra si riparte"
        );
    }
}
