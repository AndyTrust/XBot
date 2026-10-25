//! Import delle password da CSV: Bitwarden, LastPass, Firefox, 1Password, Safari/Apple,
//! Chrome/Edge. Il formato si riconosce dall'intestazione. Ogni riga diventa una voce
//! `accesso` con origine `csv:<formato>`; le righe senza password e i doppioni si saltano.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;
use serde::Serialize;
use serde_json::{Map, Value, json};
use xbot_core::{GenereCassaforte, Risposta};

use crate::cassaforte::Chiave;
use crate::cassaforte_voci::{self, DatiVoce};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Formato {
    Bitwarden,
    LastPass,
    Firefox,
    UnoPassword,
    Apple,
    Chrome,
}

impl Formato {
    pub fn nome(self) -> &'static str {
        match self {
            Self::Bitwarden => "bitwarden",
            Self::LastPass => "lastpass",
            Self::Firefox => "firefox",
            Self::UnoPassword => "1password",
            Self::Apple => "apple",
            Self::Chrome => "chrome",
        }
    }
}

/// Riconosce il formato dalle colonne (minuscole, senza spazi ai lati).
pub fn riconosci(colonne: &[String]) -> Option<Formato> {
    let ha = |n: &str| colonne.iter().any(|c| c == n);
    if ha("login_password") || ha("login_username") {
        Some(Formato::Bitwarden)
    } else if ha("grouping") || (ha("extra") && ha("password")) {
        Some(Formato::LastPass)
    } else if ha("httprealm") || ha("formactionorigin") || ha("guid") {
        Some(Formato::Firefox)
    } else if ha("title")
        && ha("password")
        && (ha("archived") || ha("tags") || ha("favorite") || ha("website"))
    {
        Some(Formato::UnoPassword)
    } else if ha("title") && ha("password") && ha("url") {
        Some(Formato::Apple)
    } else if ha("name") && ha("url") && ha("password") {
        Some(Formato::Chrome)
    } else {
        None
    }
}

/// Una riga del CSV già letta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoceCsv {
    pub titolo: String,
    pub siti: Vec<String>,
    pub utente: Option<String>,
    pub password: String,
    pub note: Option<String>,
}

/// L'esito dell'import: `{importati, saltati, errori[]}`. Gli errori non contengono valori.
#[derive(Debug, Clone, Default, Serialize)]
pub struct EsitoImport {
    pub formato: Option<&'static str>,
    pub importati: usize,
    pub saltati: usize,
    pub errori: Vec<Value>,
}

/// L'host di un URL (`https://posta.example:443/x` → `posta.example`).
fn host(url: &str) -> Option<String> {
    let senza = url.split_once("://").map_or(url, |(_, r)| r);
    let h = senza.split(['/', '?', '#']).next()?.rsplit('@').next()?;
    let h = h.split(':').next()?.trim();
    (!h.is_empty()).then(|| h.to_string())
}

fn campo(
    riga: &csv::StringRecord,
    indici: &HashMap<String, usize>,
    nomi: &[&str],
) -> Option<String> {
    nomi.iter()
        .filter_map(|n| indici.get(*n))
        .filter_map(|i| riga.get(*i))
        .map(|s| s.trim().to_string())
        .find(|s| !s.is_empty())
}

/// Una riga → voce. `Ok(None)` = da saltare (non è un accesso o non ha password).
fn voce(f: Formato, r: &csv::StringRecord, ix: &HashMap<String, usize>) -> Option<VoceCsv> {
    let c = |nomi: &[&str]| campo(r, ix, nomi);
    let (titolo, url, utente, password, note, otp) = match f {
        Formato::Bitwarden => {
            if c(&["type"]).is_some_and(|t| t != "login") {
                return None;
            }
            (
                c(&["name"]),
                c(&["login_uri"]),
                c(&["login_username"]),
                c(&["login_password"]),
                c(&["notes"]),
                c(&["login_totp"]),
            )
        }
        Formato::LastPass => {
            let url = c(&["url"]);
            if url.as_deref() == Some("http://sn") {
                return None; // nota sicura, non un accesso
            }
            (
                c(&["name"]),
                url,
                c(&["username"]),
                c(&["password"]),
                c(&["extra"]),
                c(&["totp"]),
            )
        }
        Formato::Firefox => (
            None,
            c(&["url"]),
            c(&["username"]),
            c(&["password"]),
            None,
            None,
        ),
        Formato::UnoPassword => (
            c(&["title"]),
            c(&["url", "website"]),
            c(&["username"]),
            c(&["password"]),
            c(&["notes"]),
            c(&["otpauth"]),
        ),
        Formato::Apple => (
            c(&["title"]),
            c(&["url"]),
            c(&["username"]),
            c(&["password"]),
            c(&["notes"]),
            c(&["otpauth"]),
        ),
        Formato::Chrome => (
            c(&["name"]),
            c(&["url"]),
            c(&["username"]),
            c(&["password"]),
            c(&["note"]),
            None,
        ),
    };
    let password = password?;
    let siti: Vec<String> = url
        .map(|u| {
            u.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let titolo = titolo
        .or_else(|| siti.first().and_then(|u| host(u)))
        .or_else(|| utente.clone())
        .unwrap_or_else(|| "Senza titolo".into());
    let note = match (note, otp) {
        (Some(n), Some(o)) => Some(format!("{n}\nTOTP: {o}")),
        (None, Some(o)) => Some(format!("TOTP: {o}")),
        (n, None) => n,
    };
    Some(VoceCsv {
        titolo,
        siti,
        utente,
        password,
        note,
    })
}

/// Una riga letta: `Err(numero di riga)` se illeggibile, `Ok(None)` da saltare.
pub type RigaCsv = Result<Option<VoceCsv>, usize>;

/// Legge il CSV: formato e righe.
pub fn leggi(byte: &[u8]) -> Result<(Formato, Vec<RigaCsv>), String> {
    let mut lettore = csv::ReaderBuilder::new()
        .flexible(true)
        .has_headers(true)
        .from_reader(byte);
    let colonne: Vec<String> = lettore
        .headers()
        .map_err(|_| "intestazione del CSV illeggibile".to_string())?
        .iter()
        .map(|c| c.trim_start_matches('\u{feff}').trim().to_lowercase())
        .collect();
    let formato = riconosci(&colonne)
        .ok_or_else(|| "formato del CSV non riconosciuto (Bitwarden, LastPass, Firefox, 1Password, Safari, Chrome)".to_string())?;
    let indici: HashMap<String, usize> = colonne
        .iter()
        .enumerate()
        .map(|(i, c)| (c.clone(), i))
        .collect();
    let righe = lettore
        .records()
        .enumerate()
        .map(|(n, r)| match r {
            Ok(r) => Ok(voce(formato, &r, &indici)),
            Err(_) => Err(n + 2),
        })
        .collect();
    Ok((formato, righe))
}

fn impronta(titolo: &str, utente: Option<&str>, sito: Option<&str>) -> String {
    format!(
        "{}\u{1}{}\u{1}{}",
        titolo.to_lowercase(),
        utente.unwrap_or(""),
        sito.unwrap_or("")
    )
}

/// Importa il CSV nella cassaforte. Un formato sconosciuto è un errore della richiesta.
pub fn importa(conn: &Connection, chiave: &Chiave, byte: &[u8]) -> Risposta<EsitoImport> {
    let (formato, righe) = leggi(byte).map_err(xbot_core::Errore::non_valida)?;
    let mut esistenti: HashSet<String> =
        cassaforte_voci::cerca(conn, Some(GenereCassaforte::Accesso), None)?
            .iter()
            .map(|v| {
                impronta(
                    v["titolo"].as_str().unwrap_or(""),
                    v["riferimento"]["utente"].as_str(),
                    v["siti"][0].as_str(),
                )
            })
            .collect();
    let mut esito = EsitoImport {
        formato: Some(formato.nome()),
        ..Default::default()
    };
    for (n, r) in righe.into_iter().enumerate() {
        let v = match r {
            Err(riga) => {
                esito
                    .errori
                    .push(json!({"riga": riga, "testo": "riga non leggibile"}));
                continue;
            }
            Ok(None) => {
                esito.saltati += 1;
                continue;
            }
            Ok(Some(v)) => v,
        };
        if !esistenti.insert(impronta(
            &v.titolo,
            v.utente.as_deref(),
            v.siti.first().map(String::as_str),
        )) {
            esito.saltati += 1;
            continue;
        }
        let mut rif = Map::new();
        if let Some(u) = &v.utente {
            rif.insert("utente".into(), Value::from(u.as_str()));
        }
        let mut campi = Map::new();
        campi.insert("password".into(), Value::from(v.password.as_str()));
        if let Some(n) = &v.note {
            campi.insert("note".into(), Value::from(n.as_str()));
        }
        let siti: Vec<String> = v
            .siti
            .iter()
            .take(crate::cassaforte_generi::MAX_SITI)
            .cloned()
            .collect();
        let d = DatiVoce {
            genere: Some(GenereCassaforte::Accesso),
            titolo: Some(v.titolo.chars().take(200).collect()),
            siti: Some(siti),
            riferimento: Some(rif),
            campi: Some(campi),
            assistenti: None,
            origine: Some(format!("csv:{}", formato.nome())),
        };
        match cassaforte_voci::crea(conn, chiave, &d) {
            Ok(_) => esito.importati += 1,
            Err(e) => esito
                .errori
                .push(json!({"riga": n + 2, "testo": e.codice.come_testo()})),
        }
    }
    Ok(esito)
}

#[cfg(test)]
mod test {
    use super::*;

    fn formato(intestazione: &str) -> Option<Formato> {
        let (f, _) = leggi(format!("{intestazione}\n").as_bytes()).ok()?;
        Some(f)
    }

    #[test]
    fn sei_riconoscimenti() {
        assert_eq!(
            formato(
                "folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp"
            ),
            Some(Formato::Bitwarden)
        );
        assert_eq!(
            formato("url,username,password,totp,extra,name,grouping,fav"),
            Some(Formato::LastPass)
        );
        assert_eq!(
            formato(
                "\"url\",\"username\",\"password\",\"httpRealm\",\"formActionOrigin\",\"guid\",\"timeCreated\",\"timeLastUsed\",\"timePasswordChanged\""
            ),
            Some(Formato::Firefox)
        );
        assert_eq!(
            formato("Title,Url,Username,Password,OTPAuth,Favorite,Archived,Tags,Notes"),
            Some(Formato::UnoPassword)
        );
        assert_eq!(
            formato("Title,URL,Username,Password,Notes,OTPAuth"),
            Some(Formato::Apple)
        );
        assert_eq!(
            formato("name,url,username,password,note"),
            Some(Formato::Chrome)
        );
        assert_eq!(
            formato("\u{feff}name,url,username,password"),
            Some(Formato::Chrome)
        );
        assert!(leggi(b"a,b,c\n1,2,3\n").is_err());
    }

    #[test]
    fn righe_e_doppioni() {
        let csv = "folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp\n\
            ,,login,Posta,,,0,https://posta.example,mario,p1,\n\
            ,,note,Nota,testo,,0,,,,\n\
            ,,login,,,,0,https://banca.example/entra,anna,p2,JBSW\n\
            ,,login,Senza,,,0,,x,,\n";
        let (f, righe) = leggi(csv.as_bytes()).unwrap();
        assert_eq!(f, Formato::Bitwarden);
        let v: Vec<_> = righe.into_iter().map(Result::unwrap).collect();
        assert_eq!(v[0].as_ref().unwrap().titolo, "Posta");
        assert!(v[1].is_none() && v[3].is_none());
        let b = v[2].as_ref().unwrap();
        assert_eq!(
            (b.titolo.as_str(), b.note.as_deref()),
            ("banca.example", Some("TOTP: JBSW"))
        );

        let db = crate::db_prova();
        let k = Chiave::da_byte([5u8; 32]);
        db.con(|c| {
            let e = importa(c, &k, csv.as_bytes())?;
            assert_eq!((e.importati, e.saltati, e.errori.len()), (2, 2, 0));
            let e = importa(c, &k, csv.as_bytes())?;
            assert_eq!((e.importati, e.saltati), (0, 4));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn host_degli_url() {
        assert_eq!(
            host("https://u@posta.example:443/x?y").as_deref(),
            Some("posta.example")
        );
        assert_eq!(host("posta.example").as_deref(), Some("posta.example"));
        assert_eq!(host(""), None);
    }
}
