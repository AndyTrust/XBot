//! Il testo di una mail ripulito per un assistente: niente citazioni, righe vuote ridotte, un
//! tetto di lunghezza. E il riconoscimento delle ricevute della PEC dall'oggetto.

/// Quanti caratteri di testo si restituiscono al massimo.
pub const MASSIMO_TESTO: usize = 20_000;

fn inizio_citazione(riga: &str) -> bool {
    let r = riga.trim();
    let b = r.to_lowercase();
    (b.starts_with("il giorno ") && b.ends_with("ha scritto:"))
        || (b.starts_with("on ") && b.ends_with(" wrote:"))
        || b.starts_with("-----original message")
        || b.starts_with("-----messaggio originale")
        || b.starts_with("________________________________")
        || (b.starts_with("da:") && b.contains("inviato:"))
}

/// Toglie ritorni a capo doppi, spazi in coda, citazioni (`>` e «ha scritto:») e accorcia.
pub fn ripulisci(grezzo: &str) -> (String, bool) {
    let mut fuori = String::new();
    let mut vuote = 0;
    let mut tolta = false;
    for riga in grezzo.replace('\r', "").lines() {
        if inizio_citazione(riga) {
            tolta = true;
            break;
        }
        if riga.trim_start().starts_with('>') {
            tolta = true;
            continue;
        }
        let r = riga.trim_end();
        if r.trim().is_empty() {
            vuote += 1;
            if vuote > 1 {
                continue;
            }
        } else {
            vuote = 0;
        }
        fuori.push_str(r);
        fuori.push('\n');
    }
    let mut fuori = fuori.trim().to_string();
    let mut troncato = false;
    if fuori.chars().count() > MASSIMO_TESTO {
        fuori = fuori.chars().take(MASSIMO_TESTO).collect();
        fuori.push_str("\n[…testo troncato]");
        troncato = true;
    }
    if tolta {
        fuori.push_str("\n[citazione dei messaggi precedenti tolta]");
    }
    (fuori, troncato)
}

/// Il tipo di una mail della PEC dall'oggetto: `ricevuta`, `avviso` (da agire), `messaggio`.
pub fn tipo_pec(oggetto: &str) -> Option<&'static str> {
    let o = oggetto.trim().to_uppercase();
    if o.starts_with("POSTA CERTIFICATA:") {
        Some("messaggio")
    } else if o.starts_with("ACCETTAZIONE:") || o.starts_with("CONSEGNA:") {
        Some("ricevuta")
    } else if o.starts_with("AVVISO DI MANCATA CONSEGNA")
        || o.starts_with("AVVISO DI NON ACCETTAZIONE")
        || o.starts_with("AVVISO DI RILEVAZIONE VIRUS")
    {
        Some("avviso")
    } else {
        None
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn toglie_citazioni_e_vuote() {
        let (t, _) = ripulisci(
            "Ciao\n\n\n\nGrazie  \n\nIl giorno lun 3 ott, Mario ha scritto:\n> vecchio\n",
        );
        assert_eq!(
            t,
            "Ciao\n\nGrazie\n[citazione dei messaggi precedenti tolta]"
        );
        let (t, _) = ripulisci("A\n> citato\nB");
        assert_eq!(t, "A\nB\n[citazione dei messaggi precedenti tolta]");
    }

    #[test]
    fn accorcia() {
        let (t, tr) = ripulisci(&"x".repeat(MASSIMO_TESTO + 5));
        assert!(tr && t.ends_with("[…testo troncato]"));
    }

    #[test]
    fn pec() {
        assert_eq!(tipo_pec("CONSEGNA: fattura 12"), Some("ricevuta"));
        assert_eq!(tipo_pec("AVVISO DI MANCATA CONSEGNA per x"), Some("avviso"));
        assert_eq!(tipo_pec("POSTA CERTIFICATA: ciao"), Some("messaggio"));
        assert_eq!(tipo_pec("Ciao"), None);
    }
}
