//! Fase 2.K: prima di un turno di Claude Code, oltre al rinnovo OAuth (2.I), una riga `avviso`
//! per ogni collegamento dell'assistente che gira su un'altra macchina: un collegamento del Mac
//! entra solo nel `--mcp-config` del Mac, uno stdio del server solo in quello del server.

use serde_json::json;

use super::Motore;

impl Motore {
    /// `qui`: la macchina del turno (quella del server per i turni locali).
    pub(in crate::motore) async fn s2k_prepara(&self, l: &super::Lancio, qui: &str) {
        let (assistente_id, sessione_id, turno_id) =
            (&l.assistente_id, &l.turno.sessione_id, &l.turno.id);
        self.s2i_prepara_oauth(assistente_id, sessione_id, turno_id)
            .await;
        let server = qui == self.macchine.server_id();
        let (aid, q) = (assistente_id.to_string(), qui.to_string());
        let fuori = match self
            .db
            .esegui(move |c| xbot_db::collegamenti::fuori_dal_turno(c, &aid, &q, server))
            .await
        {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(errore = %e, "collegamenti di altre macchine non letti");
                return;
            }
        };
        for (nome, macchina) in fuori {
            let dove = macchina.unwrap_or_else(|| "server".into());
            let testo = format!("il collegamento {nome} gira sul {dove}: non è in questo turno");
            self.avviso_una_volta(
                sessione_id,
                json!({ "codice": "collegamento_altrove", "testo": testo, "turno_id": turno_id }),
            )
            .await;
        }
    }
}
