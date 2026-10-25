//! Il ponte `xbot-server mcp-interno`: collega stdin/stdout di Claude Code al socket Unix del
//! server, dopo una riga d'intestazione `{sessione, turno, gettone}`.

use std::path::Path;

use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use xbot_core::{Errore, Risposta};

use super::VARIABILE_GETTONE;

/// Copia nei due sensi finché uno dei due lati chiude.
pub async fn esegui(socket: &Path, sessione: &str, turno: &str) -> Risposta<()> {
    let gettone = std::env::var(VARIABILE_GETTONE).unwrap_or_default();
    let flusso = UnixStream::connect(socket)
        .await
        .map_err(|e| Errore::interno(format!("socket {}: {e}", socket.display())))?;
    let (mut dal_server, mut al_server) = flusso.into_split();
    let intestazione = json!({ "sessione": sessione, "turno": turno, "gettone": gettone });
    al_server
        .write_all(format!("{intestazione}\n").as_bytes())
        .await
        .map_err(|e| Errore::interno(format!("intestazione: {e}")))?;
    let su = async {
        let mut stdin = tokio::io::stdin();
        let _ = tokio::io::copy(&mut stdin, &mut al_server).await;
        let _ = al_server.shutdown().await;
    };
    let giu = async {
        let mut stdout = tokio::io::stdout();
        let _ = tokio::io::copy(&mut dal_server, &mut stdout).await;
        let _ = stdout.flush().await;
    };
    // Quando il server chiude (turno finito) si esce; quando Claude chiude stdin, si aspetta
    // che il server finisca di rispondere.
    tokio::select! {
        () = giu => {}
        () = async { su.await; std::future::pending::<()>().await } => {}
    }
    Ok(())
}
