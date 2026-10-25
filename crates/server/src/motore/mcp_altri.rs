//! Fase 2.L: i collegamenti nei turni di Grok e Cursor sul server. Stessa `ConfigMcp` di Claude
//! Code ma senza il server interno di XBot: l'adattatore la scrive nel formato del suo client
//! (`xbot_provider::mcp_altri`). I permessi degli strumenti seguono il permesso della macchina.

use xbot_db::collegamenti::{self, CollegamentoVivo};
use xbot_db::macchine;
use xbot_provider::claude_code::mcp::{ConfigMcp, ServerMcp};

use super::{Lancio, Motore, nome_mcp};

/// I collegamenti come server MCP del turno, con nomi unici: (server, (nome, id collegamento)).
pub fn server_dei_collegamenti(
    coll: Vec<CollegamentoVivo>,
) -> (Vec<ServerMcp>, Vec<(String, String)>) {
    let mut nomi: Vec<String> = Vec::new();
    let mut voci = Vec::new();
    let mut server = Vec::new();
    for c in coll {
        let n = nome_mcp(&c.nome, &nomi);
        nomi.push(n.clone());
        voci.push((n.clone(), c.id.clone()));
        server.push(match c.trasporto.as_str() {
            "http" => ServerMcp::Http {
                nome: n,
                url: c.url.unwrap_or_default(),
                intestazioni: c.intestazioni,
            },
            _ => ServerMcp::Stdio {
                nome: n,
                comando: c.comando.unwrap_or_default(),
                argomenti: c.argomenti,
                ambiente: c.ambiente,
            },
        });
    }
    (server, voci)
}

impl Motore {
    /// Rinnovo OAuth, avvisi dei collegamenti di altre macchine e `richiesta.mcp` per un turno di
    /// Grok o Cursor sul server.
    pub(in crate::motore) async fn s2l_collegamenti(&self, l: &mut Lancio) {
        let srv = self.macchine.server_id();
        self.s2k_prepara(l, &srv).await;
        // Cursor non ha un canale dei permessi: sul server vale il permesso della macchina, come
        // sul Mac. Con «sempre» parte con `--force` (altrimenti `agent -p` rifiuta ogni strumento
        // MCP: «User rejected MCP»); con «chiedi» resta com'era.
        if l.turno.provider_id == xbot_provider::cursor_agent::ID {
            let m = srv.clone();
            let permesso = self
                .db
                .esegui(move |c| {
                    Ok(macchine::leggi(c, &m)?["permesso_esecuzione"]
                        .as_str()
                        .unwrap_or("ask")
                        .to_string())
                })
                .await
                .unwrap_or_else(|_| "ask".into());
            if permesso == "always" {
                l.richiesta.permessi = Some("always".into());
            }
        }
        let chiave = self.s2.chiave();
        let aid = l.assistente_id.clone();
        let r = self
            .db
            .esegui(move |c| {
                collegamenti::per_assistente_su(c, chiave.as_deref(), &aid, &srv, true)
            })
            .await;
        match r {
            Ok(coll) if !coll.is_empty() => {
                let (server, _) = server_dei_collegamenti(coll);
                l.richiesta.mcp = Some(ConfigMcp {
                    server,
                    ..Default::default()
                });
            }
            Ok(_) => {}
            Err(e) => tracing::error!(errore = %e, "collegamenti del turno non letti: turno senza"),
        }
    }
}

impl Motore {
    /// La nota in chat per un turno su un computer collegato di un client che non chiede i
    /// permessi all'utente: Grok e Cursor ricevono i collegamenti, gli altri no.
    pub(in crate::motore) async fn s2l_nota_permessi(&self, sessione_id: &str, l: &Lancio) {
        let p = l.turno.provider_id.as_str();
        if p == xbot_provider::claude_code::ID {
            return;
        }
        let (codice, t) = if xbot_provider::riceve_collegamenti(p) {
            (
                "permessi_della_macchina",
                format!(
                    "questo client non chiede i permessi: {}",
                    xbot_provider::mcp_altri::NOTA_PERMESSI
                ),
            )
        } else {
            (
                "client_senza_mcp",
                "questo client non riceve i collegamenti né chiede i permessi: comandi secondo il permesso della macchina".to_string(),
            )
        };
        let corpo = serde_json::json!({ "codice": codice, "testo": t, "turno_id": l.turno.id });
        self.avviso_una_volta(sessione_id, corpo).await;
    }
}
