//! Un client IMAP minimo, in sola lettura salvo `APPEND` (bozze e posta inviata).
//! Accesso con `AUTHENTICATE PLAIN`; cartelle aperte con `EXAMINE` (non segna nulla come letto)
//! e testi presi con `BODY.PEEK`. Niente `STORE`, niente `EXPUNGE`, niente cancellazioni.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use super::rete::{Linea, Sicurezza};

/// Il limite di un messaggio che si scarica (byte).
pub const MASSIMO_MESSAGGIO: usize = 25 * 1024 * 1024;

pub struct Imap {
    l: Linea,
    n: u32,
}

/// Una risposta `FETCH`: sequenza e voci.
#[derive(Debug, Default)]
pub struct Fetch {
    pub uid: u32,
    pub flags: Vec<String>,
    pub data_interna: String,
    pub dimensione: usize,
    /// Il contenuto del primo `BODY[...]` (intestazioni o messaggio intero).
    pub corpo: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Cartella {
    pub nome: String,
    pub attributi: Vec<String>,
}

/// Un elemento della risposta: testo o byte di un letterale.
#[derive(Debug)]
enum Pezzo {
    Testo(String),
    Letterale(Vec<u8>),
}

fn cita(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

impl Imap {
    pub async fn apri(
        host: &str,
        porta: u16,
        sic: Sicurezza,
        utente: &str,
        password: &str,
    ) -> Result<Self, String> {
        let mut l = Linea::apri(host, porta, sic).await?;
        let saluto = l.riga().await?;
        if !saluto.starts_with(b"* OK") && !saluto.starts_with(b"* PREAUTH") {
            return Err(format!(
                "saluto inatteso dal server IMAP: {}",
                String::from_utf8_lossy(&saluto).trim()
            ));
        }
        let mut c = Self { l, n: 0 };
        c.accedi(utente, password).await?;
        Ok(c)
    }

    fn tag(&mut self) -> String {
        self.n += 1;
        format!("X{}", self.n)
    }

    async fn accedi(&mut self, utente: &str, password: &str) -> Result<(), String> {
        let tag = self.tag();
        self.l
            .scrivi(format!("{tag} AUTHENTICATE PLAIN\r\n").as_bytes())
            .await?;
        let r = self.l.riga().await?;
        if !r.starts_with(b"+") {
            return Err(format!(
                "il server IMAP non accetta PLAIN: {}",
                String::from_utf8_lossy(&r).trim()
            ));
        }
        let cred = STANDARD.encode(format!("\0{utente}\0{password}"));
        self.l.scrivi(format!("{cred}\r\n").as_bytes()).await?;
        match self.fino_a_tag(&tag).await {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("accesso IMAP rifiutato ({e})")),
        }
    }

    /// Legge fino alla riga finale del comando; restituisce i pezzi delle righe non taggate,
    /// con i letterali al loro posto. Errore se la risposta non è `OK`.
    async fn fino_a_tag(&mut self, tag: &str) -> Result<Vec<Pezzo>, String> {
        let mut pezzi = Vec::new();
        loop {
            let riga = self.l.riga().await?;
            let testo = String::from_utf8_lossy(&riga).into_owned();
            if let Some(resto) = testo.strip_prefix(&format!("{tag} ")) {
                return if resto.starts_with("OK") {
                    Ok(pezzi)
                } else {
                    Err(resto.trim().chars().take(200).collect())
                };
            }
            // Letterali: `{n}\r\n` a fine riga, poi n byte, poi il resto della riga.
            let mut corrente = testo;
            loop {
                let t = corrente.trim_end().to_string();
                let marcatore = t.rfind('{').filter(|_| t.ends_with('}')).and_then(|p| {
                    Some((
                        p,
                        t[p + 1..t.len() - 1]
                            .trim_end_matches('+')
                            .parse::<usize>()
                            .ok()?,
                    ))
                });
                let Some((p, n)) = marcatore else {
                    pezzi.push(Pezzo::Testo(t));
                    break;
                };
                if n > MASSIMO_MESSAGGIO {
                    return Err("messaggio troppo grande".into());
                }
                pezzi.push(Pezzo::Testo(t[..p].to_string()));
                pezzi.push(Pezzo::Letterale(self.l.byte(n).await?));
                corrente = String::from_utf8_lossy(&self.l.riga().await?).into_owned();
            }
            pezzi.push(Pezzo::Testo("\n".into()));
        }
    }

    async fn comando(&mut self, riga: &str) -> Result<Vec<Pezzo>, String> {
        let tag = self.tag();
        self.l
            .scrivi(format!("{tag} {riga}\r\n").as_bytes())
            .await?;
        self.fino_a_tag(&tag).await
    }

    /// Apre una cartella in sola lettura; restituisce il numero di messaggi.
    pub async fn esamina(&mut self, cartella: &str) -> Result<u32, String> {
        let p = self
            .comando(&format!("EXAMINE {}", cita(cartella)))
            .await
            .map_err(|e| format!("cartella «{cartella}»: {e}"))?;
        for x in &p {
            if let Pezzo::Testo(t) = x
                && t.contains("EXISTS")
                && let Some(n) = t.trim_start_matches("* ").split_whitespace().next()
                && let Ok(n) = n.parse()
            {
                return Ok(n);
            }
        }
        Ok(0)
    }

    pub async fn cartelle(&mut self) -> Result<Vec<Cartella>, String> {
        let p = self.comando("LIST \"\" \"*\"").await?;
        let mut v = Vec::new();
        for x in p {
            let Pezzo::Testo(t) = x else { continue };
            let Some(resto) = t.strip_prefix("* LIST (") else {
                continue;
            };
            let Some((attr, dopo)) = resto.split_once(')') else {
                continue;
            };
            let nome = nome_cartella(dopo.trim());
            if !nome.is_empty() {
                v.push(Cartella {
                    nome,
                    attributi: attr.split_whitespace().map(str::to_string).collect(),
                });
            }
        }
        Ok(v)
    }

    /// `UID SEARCH` con i criteri dati (già scritti in IMAP). Restituisce gli UID crescenti.
    pub async fn cerca(&mut self, criteri: &str) -> Result<Vec<u32>, String> {
        let p = self.comando(&format!("UID SEARCH {criteri}")).await?;
        let mut v = Vec::new();
        for x in p {
            if let Pezzo::Testo(t) = x
                && let Some(r) = t.strip_prefix("* SEARCH")
            {
                v.extend(r.split_whitespace().filter_map(|n| n.parse::<u32>().ok()));
            }
        }
        v.sort_unstable();
        Ok(v)
    }

    /// `UID FETCH` di una lista di UID con `parti` (es. `BODY.PEEK[HEADER]`). Il primo letterale
    /// di ogni risposta è il corpo.
    pub async fn preleva(&mut self, uids: &[u32], parti: &str) -> Result<Vec<Fetch>, String> {
        if uids.is_empty() {
            return Ok(vec![]);
        }
        let lista = uids
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let p = self
            .comando(&format!(
                "UID FETCH {lista} (UID FLAGS INTERNALDATE RFC822.SIZE {parti})"
            ))
            .await?;
        let mut fuori: Vec<Fetch> = Vec::new();
        let mut aperto: Option<Fetch> = None;
        for x in p {
            match x {
                Pezzo::Testo(t) => {
                    if t.contains("FETCH (") && t.trim_start().starts_with('*') {
                        if let Some(f) = aperto.take() {
                            fuori.push(f);
                        }
                        aperto = Some(Fetch::default());
                    }
                    if let Some(f) = aperto.as_mut() {
                        leggi_attributi(f, &t);
                    }
                }
                Pezzo::Letterale(b) => {
                    if let Some(f) = aperto.as_mut()
                        && f.corpo.is_empty()
                    {
                        f.corpo = b;
                    }
                }
            }
        }
        if let Some(f) = aperto {
            fuori.push(f);
        }
        Ok(fuori.into_iter().filter(|f| f.uid != 0).collect())
    }

    /// Salva un messaggio in una cartella (`\Draft` per le bozze, `\Seen` per la posta inviata).
    pub async fn aggiungi(
        &mut self,
        cartella: &str,
        flag: &str,
        messaggio: &[u8],
    ) -> Result<(), String> {
        let tag = self.tag();
        self.l
            .scrivi(
                format!(
                    "{tag} APPEND {} ({flag}) {{{}}}\r\n",
                    cita(cartella),
                    messaggio.len()
                )
                .as_bytes(),
            )
            .await?;
        let r = self.l.riga().await?;
        if !r.starts_with(b"+") {
            return Err(format!(
                "APPEND rifiutato: {}",
                String::from_utf8_lossy(&r).trim()
            ));
        }
        self.l.scrivi(messaggio).await?;
        self.l.scrivi(b"\r\n").await?;
        self.fino_a_tag(&tag).await.map(|_| ())
    }

    pub async fn esci(mut self) {
        let _ = self.comando("LOGOUT").await;
    }
}

fn nome_cartella(s: &str) -> String {
    // `"/" "INBOX"` o `"." INBOX` o `"/" "[Gmail]/Bozze"`
    let mut pezzi = Vec::new();
    let mut corr = String::new();
    let mut tra_virgolette = false;
    let mut escape = false;
    for c in s.chars() {
        if escape {
            corr.push(c);
            escape = false;
        } else if c == '\\' && tra_virgolette {
            escape = true;
        } else if c == '"' {
            tra_virgolette = !tra_virgolette;
            if !tra_virgolette {
                pezzi.push(std::mem::take(&mut corr));
            }
        } else if c == ' ' && !tra_virgolette {
            if !corr.is_empty() {
                pezzi.push(std::mem::take(&mut corr));
            }
        } else {
            corr.push(c);
        }
    }
    if !corr.is_empty() {
        pezzi.push(corr);
    }
    pezzi.pop().map(|n| decodifica_utf7(&n)).unwrap_or_default()
}

/// Le cartelle con accenti arrivano in UTF-7 modificato (`&AOg-`): le rende leggibili.
pub fn decodifica_utf7(s: &str) -> String {
    let mut fuori = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '&' {
            fuori.push(c);
            continue;
        }
        let mut b64 = String::new();
        for d in it.by_ref() {
            if d == '-' {
                break;
            }
            b64.push(d);
        }
        if b64.is_empty() {
            fuori.push('&');
            continue;
        }
        let b64 = b64.replace(',', "/");
        let mut dati = STANDARD_NO_PAD
            .decode(b64.trim_end_matches('='))
            .unwrap_or_default();
        if dati.len() % 2 == 1 {
            dati.pop();
        }
        let u16s: Vec<u16> = dati
            .chunks(2)
            .map(|p| u16::from_be_bytes([p[0], p[1]]))
            .collect();
        fuori.push_str(&String::from_utf16_lossy(&u16s));
    }
    fuori
}

use base64::engine::general_purpose::STANDARD_NO_PAD;

fn leggi_attributi(f: &mut Fetch, t: &str) {
    if let Some(p) = t.find("UID ") {
        let n: String = t[p + 4..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if let Ok(n) = n.parse() {
            f.uid = n;
        }
    }
    if let Some(p) = t.find("RFC822.SIZE ") {
        let n: String = t[p + 12..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if let Ok(n) = n.parse() {
            f.dimensione = n;
        }
    }
    if let Some(p) = t.find("FLAGS (")
        && let Some(q) = t[p + 7..].find(')')
    {
        f.flags = t[p + 7..p + 7 + q]
            .split_whitespace()
            .map(str::to_string)
            .collect();
    }
    if let Some(p) = t.find("INTERNALDATE \"")
        && let Some(q) = t[p + 14..].find('"')
    {
        f.data_interna = t[p + 14..p + 14 + q].to_string();
    }
}

/// La data IMAP `d-Mon-yyyy` da `AAAA-MM-GG`.
pub fn data_imap(iso: &str) -> Result<String, String> {
    const MESI: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let p: Vec<&str> = iso.trim().split('-').collect();
    if p.len() != 3 {
        return Err(format!("data «{iso}»: serve AAAA-MM-GG"));
    }
    let (a, m, g) = (
        p[0].parse::<u32>().map_err(|_| "anno non valido")?,
        p[1].parse::<usize>().map_err(|_| "mese non valido")?,
        p[2].parse::<u32>().map_err(|_| "giorno non valido")?,
    );
    if !(1..=12).contains(&m) || !(1..=31).contains(&g) || a < 1970 {
        return Err(format!("data «{iso}» non valida"));
    }
    Ok(format!("{g}-{}-{a}", MESI[m - 1]))
}

/// Un testo da cercare, citato per IMAP. Ritorni a capo e controlli tolti.
pub fn cita_ricerca(s: &str) -> String {
    let pulito: String = s.chars().filter(|c| !c.is_control()).collect();
    cita(&pulito)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn nomi_delle_cartelle() {
        assert_eq!(nome_cartella("\"/\" \"INBOX\""), "INBOX");
        assert_eq!(nome_cartella("\".\" INBOX.Sent"), "INBOX.Sent");
        assert_eq!(
            nome_cartella("\"/\" \"[Gmail]/Posta inviata\""),
            "[Gmail]/Posta inviata"
        );
        assert_eq!(decodifica_utf7("Bozze &AOg- &- x"), "Bozze è & x");
    }

    #[test]
    fn date() {
        assert_eq!(data_imap("2026-10-05").unwrap(), "5-Oct-2026");
        assert!(data_imap("ieri").is_err());
        assert!(data_imap("2026-13-01").is_err());
    }

    #[test]
    fn citazioni() {
        assert_eq!(cita("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(cita_ricerca("ciao\r\nINBOX"), "\"ciaoINBOX\"");
    }
}
