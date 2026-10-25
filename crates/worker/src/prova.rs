//! Fase 2.K: `prova_collegamento` dal server. Il collegamento arriva già decifrato (solo in
//! memoria); la prova è quella del server (`xbot_provider::prova_mcp`) e la risposta è il frame
//! `esito_prova` con gli strumenti o il testo dell'errore (mai i segreti).

use tokio::sync::mpsc;
use xbot_provider::prova_mcp::{self, ServerDaProvare};

use crate::protocollo::{CollegamentoFilo, DalLavoratore};

fn server(c: CollegamentoFilo) -> ServerDaProvare {
    let coppie = |v: Vec<crate::protocollo::Coppia>| -> Vec<_> {
        v.into_iter().map(|c| (c.nome, c.valore)).collect()
    };
    ServerDaProvare {
        trasporto: c.trasporto,
        comando: c.comando,
        argomenti: c.argomenti,
        ambiente: coppie(c.ambiente),
        url: c.url,
        intestazioni: coppie(c.intestazioni),
    }
}

/// La prova in un compito suo; l'esito parte sulle righe grezze del canale.
pub fn avvia(grezzi: mpsc::UnboundedSender<String>, richiesta_id: String, c: CollegamentoFilo) {
    tokio::spawn(async move {
        let nome = c.nome.clone();
        let f = match prova_mcp::elenco_strumenti(&server(c)).await {
            Ok(strumenti) => DalLavoratore::EsitoProva {
                richiesta_id,
                ok: true,
                strumenti,
                errore: None,
            },
            Err(e) => DalLavoratore::EsitoProva {
                richiesta_id,
                ok: false,
                strumenti: Vec::new(),
                errore: Some(e),
            },
        };
        tracing::info!(collegamento = %nome, ok = matches!(f, DalLavoratore::EsitoProva { ok: true, .. }), "prova del collegamento");
        if let Ok(r) = serde_json::to_string(&f) {
            let _ = grezzi.send(r);
        }
    });
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::protocollo::DalServer;

    /// Il frame del server si legge e la prova gira con il server finto delle prove del server.
    #[tokio::test]
    async fn prova_collegamento_risponde_esito_prova() {
        let finto = format!(
            "{}/../server/tests/mcp-finto.py",
            env!("CARGO_MANIFEST_DIR")
        );
        let f = serde_json::json!({
            "tipo": "prova_collegamento", "richiesta_id": "r1",
            "collegamento": { "nome": "Postino", "trasporto": "stdio", "comando": "python3",
                              "argomenti": [finto],
                              "ambiente": [{ "nome": "TOKEN_FINTO", "valore": "xyz123" }] },
        });
        let DalServer::ProvaCollegamento {
            richiesta_id,
            collegamento,
        } = serde_json::from_value(f).unwrap()
        else {
            panic!("frame non letto");
        };
        let (tx, mut rx) = mpsc::unbounded_channel();
        avvia(tx, richiesta_id, *collegamento);
        let riga = tokio::time::timeout(std::time::Duration::from_secs(20), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(!riga.contains("xyz123"), "{riga}");
        let v: serde_json::Value = serde_json::from_str(&riga).unwrap();
        assert_eq!(v["tipo"], "esito_prova");
        assert_eq!(v["richiesta_id"], "r1");
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["strumenti"][0]["descrizione"], "cerca (token=xyz)");
    }
}
