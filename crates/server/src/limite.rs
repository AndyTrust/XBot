//! Il limite dei tentativi di accesso falliti, in memoria: per indirizzo del client e, come
//! rete di sicurezza contro i tentativi sparsi su molti indirizzi, per tutto il server.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
struct Conti {
    per_ip: HashMap<Option<IpAddr>, VecDeque<Instant>>,
    tutti: VecDeque<Instant>,
}

#[derive(Debug)]
pub struct Limite {
    /// Fallimenti ammessi per indirizzo nella finestra.
    massimo: usize,
    /// Fallimenti ammessi in tutto nella finestra.
    massimo_globale: usize,
    finestra: Duration,
    conti: Mutex<Conti>,
}

impl Default for Limite {
    /// 5 tentativi falliti ogni 5 minuti per indirizzo, 20 in tutto.
    fn default() -> Self {
        Self::nuovo(5, 20, Duration::from_secs(300))
    }
}

impl Limite {
    pub fn nuovo(massimo: usize, massimo_globale: usize, finestra: Duration) -> Self {
        Self {
            massimo,
            massimo_globale,
            finestra,
            conti: Mutex::new(Conti::default()),
        }
    }

    fn con<T>(&self, f: impl FnOnce(&mut Conti) -> T) -> T {
        let mut g = match self.conti.lock() {
            Ok(g) => g,
            Err(avvelenato) => avvelenato.into_inner(),
        };
        let ora = Instant::now();
        let scaduto = |t: &Instant| ora.duration_since(*t) >= self.finestra;
        let pota = |q: &mut VecDeque<Instant>| {
            while q.front().is_some_and(scaduto) {
                q.pop_front();
            }
        };
        pota(&mut g.tutti);
        g.per_ip.retain(|_, q| {
            pota(q);
            !q.is_empty()
        });
        f(&mut g)
    }

    /// Vero se nella finestra l'indirizzo (o il server) ha già raggiunto il massimo.
    pub fn bloccato(&self, ip: Option<IpAddr>) -> bool {
        self.con(|g| {
            g.tutti.len() >= self.massimo_globale
                || g.per_ip.get(&ip).is_some_and(|q| q.len() >= self.massimo)
        })
    }

    pub fn fallito(&self, ip: Option<IpAddr>) {
        self.con(|g| {
            let ora = Instant::now();
            g.tutti.push_back(ora);
            g.per_ip.entry(ip).or_default().push_back(ora);
        });
    }

    /// Un accesso riuscito azzera il conto di quell'indirizzo.
    pub fn riuscito(&self, ip: Option<IpAddr>) {
        self.con(|g| {
            g.per_ip.remove(&ip);
        });
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn blocca_e_poi_scade() {
        let a = Some("203.0.113.9".parse().unwrap());
        let b = Some("198.51.100.7".parse().unwrap());
        let l = Limite::nuovo(2, 3, Duration::from_millis(60));
        assert!(!l.bloccato(a));
        l.fallito(a);
        assert!(!l.bloccato(a));
        l.fallito(a);
        assert!(l.bloccato(a));
        assert!(!l.bloccato(b), "un altro indirizzo non è bloccato");
        l.fallito(b);
        assert!(l.bloccato(b), "il limite globale ferma tutti");
        std::thread::sleep(Duration::from_millis(80));
        assert!(!l.bloccato(a));
        l.fallito(a);
        l.riuscito(a);
        assert!(!l.bloccato(a));
    }
}
