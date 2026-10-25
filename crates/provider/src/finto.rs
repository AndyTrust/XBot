//! Il provider finto: rovescia il testo in tre pezzi, fa una chiamata di prova e finisce.
//! Con `[fallisci]` nel testo manda `Fallito`. Serve ai test senza spendere abbonamento.

use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::mpsc;
use xbot_core::Errore;

use crate::{EventoTurno, ManigliaTurno, Modello, Provider, Quota, RichiestaTurno};

pub const ID: &str = "finto";
pub const MODELLO: &str = "finto-1";
/// La quota `seven_day` che il finto dichiara a ogni turno.
pub const QUOTA_FINTA: f64 = 0.27;

#[derive(Debug, Clone)]
pub struct Finto {
    ritardo: Duration,
    /// 2.I: (comando di accesso finto, file che dice «entrato»).
    accesso: Option<(std::path::PathBuf, std::path::PathBuf)>,
}

impl Default for Finto {
    fn default() -> Self {
        Self::nuovo()
    }
}

impl Finto {
    /// Tre pezzi a 50 ms l'uno.
    pub fn nuovo() -> Self {
        Self::con_ritardo(Duration::from_millis(50))
    }

    pub fn con_ritardo(ritardo: Duration) -> Self {
        Self {
            ritardo,
            accesso: None,
        }
    }

    /// 2.I: l'accesso passa da `script` (vedi `prove/accesso-finto`); si è dentro se `file` c'è.
    pub fn con_accesso(
        mut self,
        script: impl Into<std::path::PathBuf>,
        file: impl Into<std::path::PathBuf>,
    ) -> Self {
        self.accesso = Some((script.into(), file.into()));
        self
    }
}

/// Divide il testo in tre pezzi (per caratteri, mai a metà di un carattere).
fn tre_pezzi(testo: &str) -> Vec<String> {
    let caratteri: Vec<char> = testo.chars().collect();
    let n = caratteri.len();
    let taglio = n.div_ceil(3).max(1);
    let mut pezzi: Vec<String> = caratteri
        .chunks(taglio)
        .map(|c| c.iter().collect())
        .collect();
    while pezzi.len() < 3 {
        pezzi.push(String::new());
    }
    pezzi
}

fn parole(testo: &str) -> i64 {
    i64::try_from(testo.split_whitespace().count()).unwrap_or(i64::MAX)
}

async fn turno(r: RichiestaTurno, tx: mpsc::Sender<EventoTurno>, ritardo: Duration) {
    let esterna = r
        .sessione_esterna_id
        .clone()
        .unwrap_or_else(|| format!("finto-{}", r.sessione_id));
    let avvio = EventoTurno::Avviato {
        sessione_esterna_id: Some(esterna.clone()),
    };
    if tx.send(avvio).await.is_err() {
        return;
    }
    if r.testo.contains("[fallisci]") {
        tokio::time::sleep(ritardo).await;
        let _ = tx
            .send(EventoTurno::Fallito {
                codice: "finto_fallito".into(),
                testo: "il provider finto ha fallito su richiesta".into(),
            })
            .await;
        return;
    }
    let rovescio: String = r.testo.chars().rev().collect();
    for pezzo in tre_pezzi(&rovescio) {
        tokio::time::sleep(ritardo).await;
        if tx.send(EventoTurno::TestoDelta(pezzo)).await.is_err() {
            return;
        }
    }
    let input = serde_json::json!({ "caratteri": r.testo.chars().count() });
    let eventi = [
        // L'anello della quota si prova anche senza abbonamento: 27 %, si azzera fra 4 giorni.
        EventoTurno::Quota {
            finestra: "seven_day".into(),
            utilizzo: QUOTA_FINTA,
            azzera_ms: Some(xbot_core::ora_ms() + 4 * 24 * 3600 * 1000),
        },
        EventoTurno::ChiamataStrumento {
            nome: "prova".into(),
            input_json: input,
        },
        EventoTurno::EsitoStrumento {
            nome: "prova".into(),
            esito_json: serde_json::json!({ "ok": true }),
            ok: true,
        },
        EventoTurno::Finito {
            token_ingresso: Some(parole(&r.testo)),
            token_uscita: Some(parole(&rovescio)),
            token_cache: Some(0),
            costo_usd: None,
            sessione_esterna_id: Some(esterna),
        },
    ];
    for e in eventi {
        if tx.send(e).await.is_err() {
            return;
        }
    }
}

#[async_trait]
impl Provider for Finto {
    fn id(&self) -> &str {
        ID
    }

    fn nome(&self) -> &str {
        "Finto"
    }

    fn programma(&self) -> &str {
        "interno"
    }

    async fn modelli(&self) -> Result<Vec<Modello>, Errore> {
        Ok(vec![Modello {
            id: MODELLO.into(),
            etichetta: "Finto 1".into(),
            dettaglio: Some("rovescia il testo, per le prove".into()),
            livelli: vec!["basso".into(), "alto".into()],
        }])
    }

    async fn account(&self) -> Result<Option<crate::Account>, Errore> {
        let dentro = self.accesso.as_ref().is_none_or(|(_, f)| f.exists());
        Ok(dentro.then(|| crate::Account {
            nome: "finto@prova".into(),
            dettaglio: None,
        }))
    }

    fn comando_accesso(&self) -> Option<crate::ComandoAccesso> {
        let (script, file) = self.accesso.as_ref()?;
        let s = script.to_string_lossy();
        let mut c = crate::ComandoAccesso::nuovo(std::path::Path::new("/bin/sh"), &[&s]);
        c.ambiente
            .push(("ACCESSO_FILE".into(), file.to_string_lossy().into_owned()));
        Some(c)
    }

    fn comando_uscita(&self) -> Option<crate::ComandoAccesso> {
        let (_, file) = self.accesso.as_ref()?;
        let f = file.to_string_lossy();
        Some(crate::ComandoAccesso::nuovo(
            std::path::Path::new("/bin/rm"),
            &["-f", &f],
        ))
    }

    async fn quota(&self) -> Result<Option<Quota>, Errore> {
        Ok(None)
    }

    async fn avvia(
        &self,
        richiesta: RichiestaTurno,
        eventi: mpsc::Sender<EventoTurno>,
    ) -> Result<ManigliaTurno, Errore> {
        let compito = tokio::spawn(turno(richiesta, eventi, self.ritardo));
        Ok(ManigliaTurno {
            pid: None,
            interrompi: Box::new(move || compito.abort()),
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    async fn raccogli(testo: &str) -> Vec<EventoTurno> {
        let (tx, mut rx) = mpsc::channel(16);
        let f = Finto::con_ritardo(Duration::from_millis(1));
        let r = RichiestaTurno {
            sessione_id: "s1".into(),
            testo: testo.into(),
            ..Default::default()
        };
        let _m = f.avvia(r, tx).await.unwrap();
        let mut v = Vec::new();
        while let Some(e) = rx.recv().await {
            v.push(e);
        }
        v
    }

    #[tokio::test]
    async fn rovescia_in_tre_delta_poi_finisce() {
        let v = raccogli("ciao Mario").await;
        let delta: String = v
            .iter()
            .filter_map(|e| match e {
                EventoTurno::TestoDelta(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(delta, "oiraM oaic");
        assert_eq!(
            v.iter()
                .filter(|e| matches!(e, EventoTurno::TestoDelta(_)))
                .count(),
            3
        );
        assert!(matches!(v[0], EventoTurno::Avviato { .. }));
        assert!(
            v.iter()
                .any(|e| matches!(e, EventoTurno::ChiamataStrumento { .. }))
        );
        let quattro_giorni = 4 * 24 * 3600 * 1000;
        assert!(v.iter().any(|e| matches!(
            e,
            EventoTurno::Quota { finestra, utilizzo, azzera_ms: Some(a) }
                if finestra == "seven_day"
                    && (*utilizzo - QUOTA_FINTA).abs() < 1e-9
                    && (*a - xbot_core::ora_ms() - quattro_giorni).abs() < 60_000
        )));
        assert!(matches!(
            v.last(),
            Some(EventoTurno::Finito {
                token_ingresso: Some(2),
                token_uscita: Some(2),
                ..
            })
        ));
    }

    #[tokio::test]
    async fn fallisce_a_comando() {
        let v = raccogli("prova [fallisci]").await;
        assert!(matches!(v.last(), Some(EventoTurno::Fallito { .. })));
    }

    #[tokio::test]
    async fn interrompi_chiude_il_canale() {
        let (tx, mut rx) = mpsc::channel(16);
        let f = Finto::con_ritardo(Duration::from_secs(5));
        let m = f
            .avvia(
                RichiestaTurno {
                    testo: "lungo".into(),
                    ..Default::default()
                },
                tx,
            )
            .await
            .unwrap();
        assert!(matches!(rx.recv().await, Some(EventoTurno::Avviato { .. })));
        (m.interrompi)();
        assert_eq!(rx.recv().await, None);
    }

    #[test]
    fn pezzi_corti() {
        assert_eq!(tre_pezzi(""), vec!["", "", ""]);
        assert_eq!(tre_pezzi("ab").len(), 3);
        assert_eq!(tre_pezzi("àèìòù").concat(), "àèìòù");
    }

    #[tokio::test]
    async fn modelli_e_quota() {
        let f = Finto::nuovo();
        assert_eq!(f.modelli().await.unwrap()[0].id, MODELLO);
        assert_eq!(f.quota().await.unwrap(), None);
    }
}
