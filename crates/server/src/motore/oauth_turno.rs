//! Fase 2.I: prima di un turno di Claude Code si rinnovano gli accessi OAuth dei collegamenti
//! dell'assistente. Quelli senza accesso valido restano fuori con una riga `avviso` in chat; uno
//! appena scaduto e usato da una routine manda anche la notifica «tocca a te».

use serde_json::json;

use super::Motore;

impl Motore {
    pub(in crate::motore) async fn s2i_prepara_oauth(
        &self,
        assistente_id: &str,
        sessione_id: &str,
        turno_id: &str,
    ) {
        let chiave = self.s2.chiave();
        let (fuori, scaduti) = crate::oauth::prepara_turno(&self.db, chiave, assistente_id).await;
        if !fuori.is_empty() {
            let testo = format!(
                "{}: l'accesso è scaduto o manca, ricollega l'account in Impostazioni → Collegamenti",
                fuori.join(", ")
            );
            self.avviso_una_volta(
                sessione_id,
                json!({ "codice": "collegamento_senza_accesso", "testo": testo, "turno_id": turno_id }),
            )
            .await;
        }
        for id in scaduti {
            self.avvisa_scaduto(&id).await;
        }
    }

    async fn avvisa_scaduto(&self, collegamento_id: &str) {
        let id = collegamento_id.to_string();
        let letto = self
            .db
            .esegui(move |c| {
                let nome = xbot_db::oauth::nome_collegamento(c, &id)?;
                Ok((nome, xbot_db::oauth::assistenti_con_routine(c, &id)?))
            })
            .await;
        let Ok((nome, assistenti)) = letto else {
            return;
        };
        let Some(aid) = assistenti.into_iter().next() else {
            return;
        };
        let Some(stato) = self.macchine.stato_server() else {
            return;
        };
        let n = crate::push::Notifica {
            tipo: "tocca_a_te",
            titolo: format!("{nome}: tocca a te"),
            corpo: crate::oauth::SCADUTO.to_string(),
            sessione_id: None,
            assistente_id: Some(aid),
        };
        if let Err(e) = crate::push::notifica(&stato, &reqwest::Client::new(), n).await {
            tracing::warn!(errore = %e, "notifica dell'accesso scaduto non mandata");
        }
    }
}
