//! 2.L-bis: le richieste di permesso che arrivano dal Mac (frame `richiesta` del lavoratore,
//! dal ponte `xbot-worker mcp-ponte` di Claude Code o dai permessi ACP di Grok) passano dalle
//! stesse decisioni dei turni sul server prima di arrivare all'utente: permesso dello strumento del
//! collegamento (`*lettura: sempre`, `readOnlyHint`), concessioni stabili, regole di revisione.
//! Prova vera delle 09:55: `mcp__Postino__caselle` in sola lettura con le letture «sempre»
//! chiedeva comunque il permesso in chat.

use std::path::PathBuf;

use serde_json::{Value, json};
use xbot_db::{collegamenti, sessioni};

use crate::Stato;
use crate::mcp_interno::TurnoMcp;
use crate::mcp_interno::permesso;

/// (nome nel `--mcp-config`, id) di tutti i collegamenti: il nome è quello pulito che il server
/// manda sul filo (`motore::nome_mcp`); a parità di nome vale il primo.
fn nomi_dei_collegamenti(elenco: &[Value]) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = Vec::new();
    for c in elenco {
        let (Some(nome), Some(id)) = (c["nome"].as_str(), c["id"].as_str()) else {
            continue;
        };
        let n = crate::motore::nome_mcp(nome, &[]);
        if !v.iter().any(|(x, _)| *x == n) {
            v.push((n, id.to_string()));
        }
    }
    v
}

/// Decide da solo sulla richiesta del Mac, se può: `Some((esito, motivo))` con esito
/// `approvata` o `negata`; `None`: va chiesto all'utente.
pub async fn decidi_prima(
    s: &Stato,
    sessione_id: &str,
    turno_id: Option<&str>,
    carico: &Value,
) -> Option<(String, Option<String>)> {
    let nome = carico["strumento"].as_str()?.to_string();
    let input = match &carico["input"] {
        Value::Null => json!({}),
        v => v.clone(),
    };
    let sid = sessione_id.to_string();
    let letto =
        s.db.esegui(move |c| {
            let assistente = sessioni::leggi(c, &sid)?.assistente_id;
            Ok((assistente, collegamenti::elenca(c)?))
        })
        .await;
    let (assistente_id, elenco) = match letto {
        Ok(x) => x,
        Err(e) => {
            tracing::warn!(errore = %e, "permessi della richiesta del Mac non letti: si chiede all'utente");
            return None;
        }
    };
    let t = TurnoMcp {
        turno_id: turno_id.unwrap_or_default().to_string(),
        sessione_id: sessione_id.to_string(),
        assistente_id,
        invio_id: String::new(),
        gettone: String::new(),
        passo: 0,
        cartella: PathBuf::new(),
        collegamenti: nomi_dei_collegamenti(&elenco),
    };
    let v = permesso::senza_chiedere(s, &t, &nome, &input).await?;
    if v["behavior"] == "allow" {
        tracing::info!(strumento = %nome, "richiesta del Mac approvata dai permessi dell'utente");
        Some(("approvata".into(), None))
    } else {
        let motivo = v["message"].as_str().map(str::to_string);
        tracing::info!(strumento = %nome, "richiesta del Mac negata dai permessi dell'utente");
        Some(("negata".into(), motivo))
    }
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn nomi_puliti_come_sul_filo() {
        let e = vec![
            json!({"id": "C1", "nome": "Posta (IMAP)"}),
            json!({"id": "C2", "nome": "posta imap"}),
            json!({"id": "C3", "nome": "Odoo"}),
        ];
        assert_eq!(
            nomi_dei_collegamenti(&e),
            vec![
                ("posta_imap".to_string(), "C1".to_string()),
                ("odoo".to_string(), "C3".to_string())
            ]
        );
    }
}
