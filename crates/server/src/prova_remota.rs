//! Fase 2.K: la prova di un collegamento sulla macchina dove gira. Senza `macchina_id` (o con
//! quella del server) la prova parte qui; altrimenti il server manda al lavoratore il frame
//! `prova_collegamento {richiesta_id, collegamento}` (segreti solo dentro il canale TLS) e il
//! lavoratore risponde `esito_prova {richiesta_id, ok, strumenti, errore?}`. Il codice della prova
//! è lo stesso dalle due parti: `xbot_provider::prova_mcp`.

use std::time::Duration;

use serde_json::{Value, json};
use xbot_db::collegamenti::{CollegamentoVivo, StrumentoNoto};
use xbot_provider::prova_mcp::{self, ServerDaProvare, StrumentoMcp};

use crate::stato::Stato;

/// Il lavoratore ha 30 s per la prova: qui si aspetta un po' di più.
const ATTESA: Duration = Duration::from_secs(40);

fn in_noto(t: Vec<StrumentoMcp>) -> Vec<StrumentoNoto> {
    t.into_iter()
        .map(|t| StrumentoNoto {
            nome: t.nome,
            descrizione: t.descrizione,
            sola_lettura: t.sola_lettura,
        })
        .collect()
}

fn coppie(v: &[(String, xbot_core::SegretoTesto)]) -> Vec<Value> {
    v.iter()
        .map(|(k, s)| json!({ "nome": k, "valore": s.esponi() }))
        .collect()
}

/// Il collegamento come lo legge il lavoratore (`CollegamentoFilo`).
pub fn sul_filo(c: &CollegamentoVivo) -> Value {
    json!({
        "nome": c.nome, "trasporto": c.trasporto, "url": c.url,
        "intestazioni": coppie(&c.intestazioni), "comando": c.comando,
        "argomenti": c.argomenti, "ambiente": coppie(&c.ambiente),
    })
}

/// `initialize` + `tools/list` sulla macchina giusta.
pub async fn prova(s: &Stato, c: &CollegamentoVivo) -> Result<Vec<StrumentoNoto>, String> {
    match c.macchina_id.as_deref() {
        Some(m) if m != s.servizi.macchina.id => sul_computer(s, m, c).await,
        _ => {
            let srv = ServerDaProvare {
                trasporto: c.trasporto.clone(),
                comando: c.comando.clone(),
                argomenti: c.argomenti.clone(),
                ambiente: c.ambiente.clone(),
                url: c.url.clone(),
                intestazioni: c.intestazioni.clone(),
            };
            prova_mcp::elenco_strumenti(&srv).await.map(in_noto)
        }
    }
}

async fn sul_computer(
    s: &Stato,
    macchina_id: &str,
    c: &CollegamentoVivo,
) -> Result<Vec<StrumentoNoto>, String> {
    let mid = macchina_id.to_string();
    let nome =
        s.db.esegui(move |c| xbot_db::macchine::leggi(c, &mid))
            .await
            .ok()
            .and_then(|v| v["nome"].as_str().map(str::to_string))
            .unwrap_or_else(|| macchina_id.to_string());
    let (rid, rx) = crate::accessi::attendi(s);
    let frame =
        json!({ "tipo": "prova_collegamento", "richiesta_id": rid, "collegamento": sul_filo(c) });
    if !s.motore.macchine().manda(macchina_id, &frame) {
        s.accessi.dimentica(&rid);
        return Err(format!(
            "il computer «{nome}» non è collegato: accendilo e riprova"
        ));
    }
    let v = match tokio::time::timeout(ATTESA, rx).await {
        Ok(Ok(v)) => v,
        _ => {
            s.accessi.dimentica(&rid);
            return Err(format!(
                "il computer «{nome}» non ha risposto in {} secondi",
                ATTESA.as_secs()
            ));
        }
    };
    if v["ok"].as_bool() == Some(true) {
        let t: Vec<StrumentoMcp> = serde_json::from_value(v["strumenti"].clone())
            .map_err(|e| format!("risposta del computer non valida: {e}"))?;
        Ok(in_noto(t))
    } else {
        Err(v["errore"]
            .as_str()
            .unwrap_or("prova fallita sul computer")
            .to_string())
    }
}

/// 2.K: un server MCP che non ha risposto bene ma dichiara un accesso OAuth nei metadati
/// (`/.well-known/oauth-protected-resource` o del server di autorizzazione).
pub async fn dichiara_oauth(url: &str) -> bool {
    let Ok(g) = rmcp::transport::AuthorizationManager::new(url).await else {
        return false;
    };
    let r = tokio::time::timeout(Duration::from_secs(10), g.resolve_metadata()).await;
    matches!(r, Ok(Ok(m)) if m.source.is_discovered())
}
