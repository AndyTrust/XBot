//! La rete della posta: un flusso TLS (o in chiaro, solo per l'host locale delle prove) con
//! lettura di righe e di byte esatti, usato dal client IMAP e da quello SMTP.

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::rustls::pki_types::ServerName;

/// Quanto si aspetta il server prima di rinunciare.
pub const ATTESA: Duration = Duration::from_secs(45);

/// Come ci si collega: TLS subito (993, 465) o in chiaro (solo `127.0.0.1` e `localhost`, per le
/// prove). STARTTLS non c'è: le caselle dell'utente usano tutte il TLS implicito.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sicurezza {
    Tls,
    Nessuna,
}

impl Sicurezza {
    /// La sicurezza scelta nella casella (`tls` se manca).
    pub fn scelta(s: Option<&str>) -> Result<Self, String> {
        match s.map(str::to_lowercase).as_deref() {
            None | Some("tls") | Some("ssl") => Ok(Self::Tls),
            Some("nessuna") => Ok(Self::Nessuna),
            Some(altro) => Err(format!("sicurezza «{altro}» non ammessa (solo tls)")),
        }
    }
}

pub trait Flusso: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Flusso for T {}

/// Un collegamento con lettura per righe e per byte esatti.
pub struct Linea {
    dentro: BufReader<Box<dyn Flusso>>,
}

fn connettore() -> Result<TlsConnector, String> {
    use rustls_platform_verifier::ConfigVerifierExt;
    let cfg = tokio_rustls::rustls::ClientConfig::with_platform_verifier()
        .map_err(|e| format!("verifica dei certificati: {e}"))?;
    Ok(TlsConnector::from(Arc::new(cfg)))
}

fn nome_server(host: &str) -> Result<ServerName<'static>, String> {
    ServerName::try_from(host.to_string())
        .map_err(|_| format!("nome del server non valido: {host}"))
}

async fn tls(tcp: TcpStream, host: &str) -> Result<Box<dyn Flusso>, String> {
    let s = tokio::time::timeout(ATTESA, connettore()?.connect(nome_server(host)?, tcp))
        .await
        .map_err(|_| format!("{host}: TLS scaduto"))?
        .map_err(|e| format!("{host}: TLS: {e}"))?;
    Ok(Box::new(s))
}

fn locale(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

impl Linea {
    /// Si collega.
    pub async fn apri(host: &str, porta: u16, sic: Sicurezza) -> Result<Self, String> {
        if sic == Sicurezza::Nessuna && !locale(host) {
            return Err(format!("{host}: la connessione in chiaro non è ammessa"));
        }
        let tcp = tokio::time::timeout(ATTESA, TcpStream::connect((host, porta)))
            .await
            .map_err(|_| format!("{host}:{porta}: connessione scaduta"))?
            .map_err(|e| format!("{host}:{porta}: {e}"))?;
        let flusso: Box<dyn Flusso> = match sic {
            Sicurezza::Tls => tls(tcp, host).await?,
            _ => Box::new(tcp),
        };
        Ok(Self {
            dentro: BufReader::new(flusso),
        })
    }

    pub async fn riga(&mut self) -> Result<Vec<u8>, String> {
        let mut v = Vec::new();
        let n = tokio::time::timeout(ATTESA, self.dentro.read_until(b'\n', &mut v))
            .await
            .map_err(|_| "il server non risponde".to_string())?
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("il server ha chiuso il collegamento".into());
        }
        if v.len() > 1_000_000 {
            return Err("riga troppo lunga".into());
        }
        Ok(v)
    }

    pub async fn byte(&mut self, n: usize) -> Result<Vec<u8>, String> {
        let mut v = vec![0u8; n];
        tokio::time::timeout(ATTESA * 4, self.dentro.read_exact(&mut v))
            .await
            .map_err(|_| "il server non risponde".to_string())?
            .map_err(|e| e.to_string())?;
        Ok(v)
    }

    pub async fn scrivi(&mut self, dati: &[u8]) -> Result<(), String> {
        let s = self.dentro.get_mut();
        tokio::time::timeout(ATTESA, async {
            s.write_all(dati).await?;
            s.flush().await
        })
        .await
        .map_err(|_| "scrittura scaduta".to_string())?
        .map_err(|e| e.to_string())
    }
}
