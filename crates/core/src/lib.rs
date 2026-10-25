//! Tipi condivisi di XBot: id, tempi, segreti, errori ed enumerazioni dello schema.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod enumerazioni;
mod errore;
mod segreto;

pub use enumerazioni::{
    Consegna, GenereCassaforte, GenereRiga, PermessoEsecuzione, Ruolo, StatoInvio, StatoLavoro,
    StatoRichiesta, StatoTurno, TipoRichiesta,
};
pub use errore::{Codice, Errore, Risposta};
pub use segreto::SegretoTesto;

/// Id testuale (ULID) come lo vuole lo schema.
pub type Id = String;

/// Millisecondi dall'epoca.
pub type Ms = i64;

/// Un nuovo id ULID.
pub fn nuovo_id() -> Id {
    ulid::Ulid::generate().to_string()
}

/// L'ora corrente in millisecondi dall'epoca.
pub fn ora_ms() -> Ms {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// La versione del motore, come la dice Cargo.
pub const VERSIONE: &str = env!("CARGO_PKG_VERSION");

/// Il nome del bot orchestratore (`XBOT_NOME`, predefinito «XBot»): nome della voce
/// predefinita e di ripiego nei testi.
pub fn nome_bot() -> String {
    std::env::var("XBOT_NOME")
        .ok()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "XBot".into())
}

/// La versione in opera: la prima parola di `XBOT_VERSIONE_FILE` (predefinito
/// `<dati>/versione.txt`, scritto da `installa.sh`), altrimenti la versione di Cargo.
pub fn versione_installata(dati: &std::path::Path) -> String {
    let file = std::env::var_os("XBOT_VERSIONE_FILE")
        .filter(|f| !f.is_empty())
        .map_or_else(|| dati.join("versione.txt"), std::path::PathBuf::from);
    versione_da_testo(&std::fs::read_to_string(file).unwrap_or_default())
        .unwrap_or_else(|| VERSIONE.to_string())
}

/// La prima parola del testo di `versione.txt` (`<tag o hash> <AAAA-MM-GG HH:MM>`).
pub fn versione_da_testo(t: &str) -> Option<String> {
    t.split_whitespace().next().map(str::to_string)
}

/// La cartella di XBot nella casa dell'utente (`~/.xbot`): base dei predefiniti del
/// lavoratore e dei provider quando `XBOT_DATI` non c'è.
pub fn cartella_casa() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map_or_else(std::env::temp_dir, std::path::PathBuf::from)
        .join(".xbot")
}

/// La cartella dei dati: `XBOT_DATI` se c'è, altrimenti `~/.xbot`.
pub fn cartella_dati_predefinita() -> std::path::PathBuf {
    std::env::var_os("XBOT_DATI")
        .filter(|d| !d.is_empty())
        .map_or_else(cartella_casa, std::path::PathBuf::from)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn versione_dal_file() {
        assert_eq!(
            versione_da_testo("v2026.10.25 2026-10-25 17:40\n").as_deref(),
            Some("v2026.10.25")
        );
        assert_eq!(versione_da_testo("  \n"), None);
        let dir = std::env::temp_dir().join(format!("xbot-versione-{}", nuovo_id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(versione_installata(&dir), VERSIONE);
        std::fs::write(dir.join("versione.txt"), "abc1234 2026-10-25 17:40\n").unwrap();
        assert_eq!(versione_installata(&dir), "abc1234");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn id_e_ora() {
        let a = nuovo_id();
        let b = nuovo_id();
        assert_eq!(a.len(), 26);
        assert_ne!(a, b);
        assert!(ora_ms() > 1_700_000_000_000);
    }
}
