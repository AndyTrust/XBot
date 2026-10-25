//! Il bus per sessione: porta solo il segnale «è cambiato qualcosa»; i dati si rileggono dal database.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

#[derive(Debug, Clone, PartialEq)]
pub enum Segnale {
    /// Una riga nuova o riscritta.
    Righe { aggiornata_seq: i64 },
    /// La cronologia è stata azzerata.
    Azzerata { generazione: i64 },
    /// Il turno ha cambiato stato.
    Turno { turno_id: String, stato: String },
    /// Una richiesta della sessione è nata o ha cambiato stato.
    Richiesta {
        id: String,
        tipo: String,
        stato: String,
    },
    /// Non letti o «in attesa» dell'assistente sono cambiati: l'interfaccia rilegge la barra.
    Barra,
}

#[derive(Clone, Default)]
pub struct Bus {
    canali: Arc<Mutex<HashMap<String, broadcast::Sender<Segnale>>>>,
}

impl Bus {
    pub fn iscrivi(&self, sessione_id: &str) -> broadcast::Receiver<Segnale> {
        let mut canali = match self.canali.lock() {
            Ok(c) => c,
            Err(avvelenato) => avvelenato.into_inner(),
        };
        canali
            .entry(sessione_id.to_string())
            .or_insert_with(|| broadcast::channel(256).0)
            .subscribe()
    }

    pub fn pubblica(&self, sessione_id: &str, segnale: Segnale) {
        let mut canali = match self.canali.lock() {
            Ok(c) => c,
            Err(avvelenato) => avvelenato.into_inner(),
        };
        if let Some(tx) = canali.get(sessione_id) {
            if tx.receiver_count() == 0 {
                canali.remove(sessione_id);
            } else {
                let _ = tx.send(segnale);
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn segnale_arriva_solo_alla_sessione_giusta() {
        let bus = Bus::default();
        let mut a = bus.iscrivi("a");
        let mut b = bus.iscrivi("b");
        bus.pubblica("a", Segnale::Righe { aggiornata_seq: 3 });
        assert_eq!(
            a.recv().await.unwrap(),
            Segnale::Righe { aggiornata_seq: 3 }
        );
        assert!(b.try_recv().is_err());
        bus.pubblica("nessuno", Segnale::Azzerata { generazione: 2 });
    }
}
