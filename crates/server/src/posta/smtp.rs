//! Un client SMTP minimo con TLS implicito (porta 465): `AUTH PLAIN`, un messaggio, `QUIT`.
//! Si chiama solo dopo il sì dell'utente alla richiesta in chat.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use super::rete::{Linea, Sicurezza};

async fn risposta(l: &mut Linea) -> Result<(u16, String), String> {
    let mut testo = String::new();
    loop {
        let r = l.riga().await?;
        let r = String::from_utf8_lossy(&r).trim_end().to_string();
        let codice: u16 = r
            .get(..3)
            .and_then(|c| c.parse().ok())
            .ok_or_else(|| format!("risposta SMTP illeggibile: {r}"))?;
        testo.push_str(&r);
        testo.push('\n');
        if r.as_bytes().get(3) != Some(&b'-') {
            return Ok((codice, testo));
        }
    }
}

async fn attendi(l: &mut Linea, giusti: &[u16], cosa: &str) -> Result<String, String> {
    let (c, t) = risposta(l).await?;
    if giusti.contains(&c) {
        Ok(t)
    } else {
        Err(format!(
            "{cosa}: {}",
            t.trim().chars().take(240).collect::<String>()
        ))
    }
}

async fn manda(l: &mut Linea, riga: &str) -> Result<(), String> {
    l.scrivi(format!("{riga}\r\n").as_bytes()).await
}

/// Il nome con cui ci si presenta al server.
fn nome_locale() -> &'static str {
    "xbot.local"
}

/// Il server di posta in uscita e l'accesso.
pub struct Server<'a> {
    pub host: &'a str,
    pub porta: u16,
    pub sic: Sicurezza,
    pub utente: &'a str,
    pub password: &'a str,
}

/// Spedisce `messaggio` (RFC 5322, già completo) da `da` a tutti i `destinatari`.
pub async fn spedisci(
    srv: &Server<'_>,
    da: &str,
    destinatari: &[String],
    messaggio: &[u8],
) -> Result<(), String> {
    if destinatari.is_empty() {
        return Err("nessun destinatario".into());
    }
    let mut l = Linea::apri(srv.host, srv.porta, srv.sic).await?;
    attendi(&mut l, &[220], "saluto SMTP").await?;
    manda(&mut l, &format!("EHLO {}", nome_locale())).await?;
    attendi(&mut l, &[250], "EHLO").await?;
    let cred = STANDARD.encode(format!("\0{}\0{}", srv.utente, srv.password));
    manda(&mut l, &format!("AUTH PLAIN {cred}")).await?;
    attendi(&mut l, &[235], "accesso SMTP rifiutato").await?;
    manda(&mut l, &format!("MAIL FROM:<{}>", indirizzo_pulito(da)?)).await?;
    attendi(&mut l, &[250], "MAIL FROM").await?;
    for d in destinatari {
        manda(&mut l, &format!("RCPT TO:<{}>", indirizzo_pulito(d)?)).await?;
        attendi(&mut l, &[250, 251], &format!("destinatario {d}")).await?;
    }
    manda(&mut l, "DATA").await?;
    attendi(&mut l, &[354], "DATA").await?;
    l.scrivi(&con_punti(messaggio)).await?;
    attendi(&mut l, &[250], "invio del messaggio").await?;
    let _ = manda(&mut l, "QUIT").await;
    Ok(())
}

/// L'indirizzo nudo, senza controlli o parentesi che aprirebbero comandi.
pub fn indirizzo_pulito(s: &str) -> Result<String, String> {
    let s = s.trim();
    let s = match (s.rfind('<'), s.rfind('>')) {
        (Some(a), Some(b)) if a < b => &s[a + 1..b],
        _ => s,
    };
    let ok = !s.is_empty()
        && s.matches('@').count() == 1
        && !s.starts_with('@')
        && !s.ends_with('@')
        && s.chars().all(|c| {
            !c.is_control() && !c.is_whitespace() && !matches!(c, '<' | '>' | '"' | ',' | ';')
        });
    if ok {
        Ok(s.to_string())
    } else {
        Err(format!("indirizzo non valido: «{s}»"))
    }
}

/// Fine riga CRLF, righe che iniziano con `.` raddoppiate, chiusura `.\r\n`.
fn con_punti(m: &[u8]) -> Vec<u8> {
    let mut fuori = Vec::with_capacity(m.len() + 16);
    let mut inizio_riga = true;
    let mut i = 0;
    while i < m.len() {
        let b = m[i];
        if inizio_riga && b == b'.' {
            fuori.push(b'.');
        }
        if b == b'\n' && fuori.last() != Some(&b'\r') {
            fuori.push(b'\r');
        }
        fuori.push(b);
        inizio_riga = b == b'\n';
        i += 1;
    }
    if !fuori.ends_with(b"\r\n") {
        fuori.extend_from_slice(b"\r\n");
    }
    fuori.extend_from_slice(b".\r\n");
    fuori
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn punti_e_fine_riga() {
        let v = con_punti(b"ciao\n.nascosto\r\n..due\n");
        assert_eq!(v, b"ciao\r\n..nascosto\r\n...due\r\n.\r\n");
    }

    #[test]
    fn indirizzi() {
        assert_eq!(indirizzo_pulito("Mario <a@b.it>").unwrap(), "a@b.it");
        assert_eq!(indirizzo_pulito(" a@b.it ").unwrap(), "a@b.it");
        assert!(indirizzo_pulito("a@b.it>\r\nRCPT TO:<x@y.it").is_err());
        assert!(indirizzo_pulito("senza-chiocciola").is_err());
        assert!(indirizzo_pulito("a@b@c").is_err());
    }
}
