//! La chiusura di un turno: stato del turno, stato dell'invio, segnali della barra, annuncio.
//!
//! Regole (Fase 2.B-bis):
//! - `turno.errore_codice` è il codice del `Fallito` del provider (es. `finto_fallito`).
//! - `invio.stato` diventa `consegnato` quando il turno finisce, comunque finisca (riuscito,
//!   fallito, interrotto); `fallito` solo se il provider non è mai partito.

use xbot_core::{StatoInvio, StatoTurno};
use xbot_db::invii;
use xbot_db::lettura;
use xbot_db::turni::{self, Conto};

use super::{Lancio, Motore};
use crate::bus::Segnale;
use crate::traduttore::{Fine, Traduttore};

impl Motore {
    /// Chiude il turno e restituisce il prossimo invio in coda (o `None`: sessione libera).
    pub(super) async fn chiudi_turno(
        &self,
        sessione_id: &str,
        l: &Lancio,
        trad: &Traduttore,
        fine: Option<Fine>,
        partito: bool,
    ) -> Option<String> {
        let dal_provider = fine.is_some();
        let fine = fine.unwrap_or(Fine::Fallito {
            codice: "provider_chiuso".into(),
            testo: "il provider ha chiuso senza finire il turno".into(),
        });
        let (stato, conto, errore) = match &fine {
            Fine::Finito {
                token_ingresso,
                token_uscita,
                token_cache,
                costo_usd,
            } => (
                StatoTurno::Riuscito,
                Conto {
                    token_ingresso: *token_ingresso,
                    token_uscita: *token_uscita,
                    token_cache: *token_cache,
                    costo_usd: *costo_usd,
                },
                None,
            ),
            Fine::Fallito { codice, testo } => (
                StatoTurno::Fallito,
                Conto::default(),
                Some((codice.clone(), testo.clone())),
            ),
        };
        let stato_invio = if partito {
            StatoInvio::Consegnato
        } else {
            StatoInvio::Fallito
        };
        let (tid, iid, aid) = (
            l.turno.id.clone(),
            l.invio_id.clone(),
            l.assistente_id.clone(),
        );
        let esito = self
            .db
            .esegui(move |c| {
                let err = errore.as_ref().map(|(k, t)| (k.as_str(), t.as_str()));
                let chiuso = turni::chiudi(c, &tid, stato, &conto, err)?;
                // Anche un turno già chiuso come interrotto porta l'invio al suo stato finale.
                invii::imposta_stato(c, &iid, stato_invio)?;
                let finale = turni::leggi(c, &tid)?.stato;
                // «Tocca a te» dopo una risposta riuscita; si spegne con «letto» o un invio.
                let riuscito = finale == StatoTurno::Riuscito.come_testo();
                lettura::imposta_lavoro(c, &aid, false, riuscito)?;
                Ok((chiuso, finale))
            })
            .await;
        let stato_finale = match esito {
            Ok((chiuso, s)) => {
                if chiuso
                    && !dal_provider
                    && let Fine::Fallito { codice, testo } = &fine
                    && let Err(e) = trad.avviso_errore(codice, testo).await
                {
                    tracing::error!(errore = %e, "avviso non scritto");
                }
                s
            }
            Err(e) => {
                tracing::error!(errore = %e, "turno non chiuso");
                StatoTurno::Fallito.come_testo().into()
            }
        };
        tracing::info!(turno = %l.turno.id, stato = %stato_finale, "turno chiuso");
        // Prima si libera la sessione (o si prende il prossimo), poi si annuncia la fine:
        // chi vede «riuscito» trova la sessione già libera.
        let prossimo = self.prossimo_o_libera(sessione_id);
        self.bus.pubblica(
            sessione_id,
            Segnale::Turno {
                turno_id: l.turno.id.clone(),
                stato: stato_finale,
            },
        );
        // Sessioni figlie: stato nel padre e, a lavoro finito, il risultato che risale.
        if let Some(k) =
            crate::mcp_interno::figlie::sincronizza(&self.db, &self.bus, sessione_id, true).await
        {
            tokio::spawn(crate::mcp_interno::figlie::riporta(self.clone(), k));
        }
        prossimo
    }
}
