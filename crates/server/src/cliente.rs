//! L'indirizzo del client, anche dietro un reverse proxy fidato (`XBOT_PROXY_FIDATO`).

use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::HeaderMap;
use axum::http::request::Parts;
use xbot_core::{Errore, Risposta};

use crate::stato::Stato;

/// Una rete fidata: un indirizzo con il suo prefisso (`172.16.0.0/12`, `127.0.0.1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rete {
    base: IpAddr,
    prefisso: u8,
}

impl Rete {
    pub fn leggi(testo: &str) -> Risposta<Rete> {
        let t = testo.trim();
        let errore = || Errore::non_valida(format!("rete fidata non valida: «{t}»"));
        let (ip, prefisso) = match t.split_once('/') {
            Some((ip, p)) => (ip, Some(p.parse::<u8>().map_err(|_| errore())?)),
            None => (t, None),
        };
        let base = canonico(ip.parse::<IpAddr>().map_err(|_| errore())?);
        let massimo = if base.is_ipv4() { 32 } else { 128 };
        let prefisso = prefisso.unwrap_or(massimo);
        if prefisso > massimo {
            return Err(errore());
        }
        Ok(Rete { base, prefisso })
    }

    /// Una lista separata da virgole; le voci vuote si saltano.
    pub fn leggi_lista(testo: &str) -> Risposta<Vec<Rete>> {
        testo
            .split(',')
            .filter(|v| !v.trim().is_empty())
            .map(Rete::leggi)
            .collect()
    }

    pub fn contiene(&self, ip: IpAddr) -> bool {
        match (self.base, canonico(ip)) {
            (IpAddr::V4(b), IpAddr::V4(i)) => maschera(
                u128::from(u32::from(b)),
                u128::from(u32::from(i)),
                self.prefisso,
                32,
            ),
            (IpAddr::V6(b), IpAddr::V6(i)) => {
                maschera(u128::from(b), u128::from(i), self.prefisso, 128)
            }
            _ => false,
        }
    }
}

impl std::fmt::Display for Rete {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.base, self.prefisso)
    }
}

fn maschera(base: u128, ip: u128, prefisso: u8, bit: u8) -> bool {
    let via = u32::from(bit - prefisso);
    base.checked_shr(via).unwrap_or(0) == ip.checked_shr(via).unwrap_or(0)
}

/// `::ffff:1.2.3.4` vale come `1.2.3.4`.
fn canonico(ip: IpAddr) -> IpAddr {
    ip.to_canonical()
}

/// Un valore di `X-Forwarded-For`: `1.2.3.4`, `1.2.3.4:5678`, `::1`, `[::1]:5678`.
fn ip_da_voce(v: &str) -> Option<IpAddr> {
    let v = v.trim();
    v.parse::<IpAddr>()
        .ok()
        .or_else(|| v.parse::<SocketAddr>().ok().map(|s| s.ip()))
        .map(canonico)
}

/// L'indirizzo del client. Se la connessione arriva da un proxy fidato vale l'ultimo indirizzo
/// non fidato di `X-Forwarded-For` (o `X-Real-IP` se c'è solo quello); altrimenti quello della
/// connessione. Senza proxy fidati le intestazioni si ignorano.
pub fn ip_cliente(connessione: Option<IpAddr>, h: &HeaderMap, fidati: &[Rete]) -> Option<IpAddr> {
    let conn = connessione.map(canonico);
    let fidato = |ip: IpAddr| fidati.iter().any(|r| r.contiene(ip));
    let Some(c) = conn.filter(|c| fidato(*c)) else {
        return conn;
    };
    let voci: Vec<&str> = h
        .get_all("x-forwarded-for")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .filter(|v| !v.trim().is_empty())
        .collect();
    if voci.is_empty() {
        return Some(
            h.get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .and_then(ip_da_voce)
                .unwrap_or(c),
        );
    }
    let mut ultimo = c;
    for v in voci.iter().rev() {
        // Una voce illeggibile ferma la catena: si tiene l'ultimo indirizzo sicuro.
        let Some(ip) = ip_da_voce(v) else {
            return Some(ultimo);
        };
        if !fidato(ip) {
            return Some(ip);
        }
        ultimo = ip;
    }
    Some(ultimo)
}

/// Estrattore: l'indirizzo del client per il limite e per il log (`None` nelle prove senza rete).
#[derive(Debug, Clone, Copy)]
pub struct IpCliente(pub Option<IpAddr>);

impl std::fmt::Display for IpCliente {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Some(ip) => write!(f, "{ip}"),
            None => f.write_str("sconosciuto"),
        }
    }
}

impl FromRequestParts<Stato> for IpCliente {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, s: &Stato) -> Result<Self, Infallible> {
        let conn = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip());
        Ok(IpCliente(ip_cliente(conn, &parts.headers, &s.proxy_fidati)))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use axum::http::HeaderValue;

    fn ip(t: &str) -> IpAddr {
        t.parse().unwrap()
    }

    fn intestazioni(coppie: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in coppie {
            h.append(*k, HeaderValue::from_static(v));
        }
        h
    }

    #[test]
    fn reti() {
        let l = Rete::leggi_lista("172.16.0.0/12, 127.0.0.1,,::1").unwrap();
        assert_eq!(l.len(), 3);
        assert!(l[0].contiene(ip("172.20.1.2")));
        assert!(!l[0].contiene(ip("172.32.0.1")));
        assert!(l[1].contiene(ip("127.0.0.1")));
        assert!(l[1].contiene(ip("::ffff:127.0.0.1")));
        assert!(!l[1].contiene(ip("127.0.0.2")));
        assert!(l[2].contiene(ip("::1")));
        assert!(Rete::leggi("0.0.0.0/0").unwrap().contiene(ip("8.8.8.8")));
        assert!(Rete::leggi("10.0.0.0/33").is_err());
        assert!(Rete::leggi("non-un-ip").is_err());
    }

    #[test]
    fn cliente_dietro_proxy() {
        let fidati = Rete::leggi_lista("127.0.0.1,172.16.0.0/12").unwrap();
        let locale = Some(ip("127.0.0.1"));
        let h = intestazioni(&[("x-forwarded-for", "203.0.113.9")]);
        assert_eq!(ip_cliente(locale, &h, &fidati), Some(ip("203.0.113.9")));
        // Senza proxy fidati l'intestazione si ignora.
        assert_eq!(ip_cliente(locale, &h, &[]), locale);
        // Da una connessione non fidata l'intestazione si ignora.
        let fuori = Some(ip("198.51.100.7"));
        assert_eq!(ip_cliente(fuori, &h, &fidati), fuori);
        // Vale l'ultimo non fidato: il primo valore lo può scrivere chiunque.
        let h = intestazioni(&[
            ("x-forwarded-for", "1.1.1.1, 203.0.113.9"),
            ("x-forwarded-for", "172.17.0.1"),
        ]);
        assert_eq!(ip_cliente(locale, &h, &fidati), Some(ip("203.0.113.9")));
        // Tutti fidati: il più lontano.
        let h = intestazioni(&[("x-forwarded-for", "172.18.0.5, 172.17.0.1")]);
        assert_eq!(ip_cliente(locale, &h, &fidati), Some(ip("172.18.0.5")));
        // Solo X-Real-IP.
        let h = intestazioni(&[("x-real-ip", "203.0.113.10")]);
        assert_eq!(ip_cliente(locale, &h, &fidati), Some(ip("203.0.113.10")));
        // Con porta e IPv6.
        let h = intestazioni(&[("x-forwarded-for", "[2001:db8::1]:443")]);
        assert_eq!(ip_cliente(locale, &h, &fidati), Some(ip("2001:db8::1")));
        // Voce illeggibile: si tiene la connessione.
        let h = intestazioni(&[("x-forwarded-for", "boh")]);
        assert_eq!(ip_cliente(locale, &h, &fidati), locale);
        assert_eq!(ip_cliente(None, &h, &fidati), None);
    }
}
