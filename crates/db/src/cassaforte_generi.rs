//! Le regole per genere della cassaforte (contratto 2.D, sezione 1): quali campi sono ammessi
//! nel riferimento (in chiaro) e nei campi (cifrati), Luhn, marca e ultime quattro cifre delle
//! carte, nome della variabile dei segreti.

use serde_json::{Map, Value};
use xbot_core::{Codice, Errore, GenereCassaforte, Risposta};

/// Siti al massimo per voce, e lunghezza massima di ciascuno.
pub const MAX_SITI: usize = 20;
const MAX_SITO: usize = 2000;
const MAX_TESTO: usize = 10_000;

/// Le chiavi ammesse: (riferimento, campi).
fn chiavi(genere: GenereCassaforte) -> (&'static [&'static str], &'static [&'static str]) {
    match genere {
        GenereCassaforte::Accesso => (&["utente"], &["password", "pin", "passphrase", "note"]),
        GenereCassaforte::Carta => (
            &[
                "brand",
                "last4",
                "exp_month",
                "exp_year",
                "intestatario",
                "nome_breve",
            ],
            &["numero", "cvc", "indirizzo"],
        ),
        GenereCassaforte::Contatto => (&["nome", "email", "telefono", "indirizzo"], &[]),
        GenereCassaforte::Segreto => (&["variabile", "descrizione"], &["valore"]),
    }
}

fn non_valida(t: impl Into<String>) -> Errore {
    Errore::non_valida(t)
}

/// Toglie le chiavi nulle e rifiuta quelle sconosciute o troppo lunghe.
fn pulisci(m: &Map<String, Value>, ammesse: &[&str], dove: &str) -> Risposta<Map<String, Value>> {
    let mut fuori = Map::new();
    for (k, v) in m {
        if !ammesse.contains(&k.as_str()) {
            return Err(non_valida(format!("{dove}: campo non ammesso «{k}»")));
        }
        match v {
            Value::Null => {}
            Value::String(s) if s.trim().is_empty() => {}
            Value::String(s) if s.chars().count() > MAX_TESTO => {
                return Err(Errore::nuovo(
                    Codice::TestoTroppoLungo,
                    format!("{dove}.{k}: al massimo {MAX_TESTO} caratteri"),
                ));
            }
            altro => {
                fuori.insert(k.clone(), altro.clone());
            }
        }
    }
    Ok(fuori)
}

fn testo<'a>(m: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    m.get(k).and_then(Value::as_str).map(str::trim)
}

fn richiedi_testo(m: &Map<String, Value>, k: &str, dove: &str) -> Risposta<String> {
    match m.get(k) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.clone()),
        Some(Value::String(_)) | None => Err(non_valida(format!("{dove}: «{k}» è obbligatorio"))),
        Some(_) => Err(non_valida(format!("{dove}: «{k}» vuole un testo"))),
    }
}

/// I siti: puliti, al massimo `MAX_SITI`, ciascuno al massimo 2000 caratteri.
pub fn siti(v: &[String]) -> Risposta<Vec<String>> {
    let s: Vec<String> = v
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if s.len() > MAX_SITI {
        return Err(non_valida(format!("al massimo {MAX_SITI} siti")));
    }
    if s.iter().any(|x| x.chars().count() > MAX_SITO) {
        return Err(Errore::nuovo(
            Codice::TestoTroppoLungo,
            format!("un sito supera {MAX_SITO} caratteri"),
        ));
    }
    Ok(s)
}

/// Normalizza riferimento e campi di un genere. Restituisce (riferimento, campi) pronti da
/// salvare: per le carte `brand` e `last4` li ricava il server dal numero.
pub fn normalizza(
    genere: GenereCassaforte,
    riferimento: &Map<String, Value>,
    campi: &Map<String, Value>,
) -> Risposta<(Map<String, Value>, Map<String, Value>)> {
    let (k_rif, k_campi) = chiavi(genere);
    let mut rif = pulisci(riferimento, k_rif, "riferimento")?;
    let mut cam = pulisci(campi, k_campi, "campi")?;
    match genere {
        GenereCassaforte::Accesso => {
            richiedi_testo(&cam, "password", "campi")?;
        }
        GenereCassaforte::Carta => carta(&mut rif, &mut cam)?,
        GenereCassaforte::Contatto => {}
        GenereCassaforte::Segreto => {
            let v = richiedi_testo(&rif, "variabile", "riferimento")?;
            if !variabile_valida(v.trim()) {
                return Err(non_valida(
                    "riferimento.variabile: lettere maiuscole, cifre e _ (es. GITHUB_TOKEN)",
                ));
            }
            if variabile_vietata(v.trim()) {
                return Err(Errore::nuovo(
                    Codice::VariabileNonAmmessa,
                    "riferimento.variabile: PATH, HOME, LD_*, DYLD_*, XBOT_*, ANTHROPIC_*, CLAUDE_* non ammesse",
                ));
            }
            rif.insert("variabile".into(), Value::from(v.trim()));
            richiedi_testo(&cam, "valore", "campi")?;
        }
    }
    for v in cam.values_mut() {
        if let Value::Number(n) = v {
            *v = Value::String(n.to_string());
        }
    }
    Ok((rif, cam))
}

/// `NOME_VARIABILE`: `[A-Z][A-Z0-9_]*`, al massimo 64.
pub fn variabile_valida(v: &str) -> bool {
    let mut c = v.chars();
    matches!(c.next(), Some(p) if p.is_ascii_uppercase())
        && v.len() <= 64
        && c.all(|x| x.is_ascii_uppercase() || x.is_ascii_digit() || x == '_')
}

/// Le variabili che un segreto non può prendere: cambierebbero il processo del provider.
pub fn variabile_vietata(v: &str) -> bool {
    const ESATTE: [&str; 4] = ["PATH", "HOME", "LD_PRELOAD", "LD_LIBRARY_PATH"];
    const PREFISSI: [&str; 4] = ["DYLD_", "XBOT_", "ANTHROPIC_", "CLAUDE_"];
    ESATTE.contains(&v) || PREFISSI.iter().any(|p| v.starts_with(p))
}

fn carta(rif: &mut Map<String, Value>, cam: &mut Map<String, Value>) -> Risposta<()> {
    let numero: String = richiedi_testo(cam, "numero", "campi")?
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    if !(12..=19).contains(&numero.len()) || !numero.chars().all(|c| c.is_ascii_digit()) {
        return Err(Errore::nuovo(
            Codice::CartaNonValida,
            "il numero della carta vuole da 12 a 19 cifre",
        ));
    }
    if !luhn(&numero) {
        return Err(Errore::nuovo(
            Codice::CartaNonValida,
            "il numero della carta non supera il controllo di Luhn",
        ));
    }
    if let Some(cvc) = testo(cam, "cvc")
        && !(cvc.len() == 3 || cvc.len() == 4)
    {
        return Err(Errore::nuovo(
            Codice::CartaNonValida,
            "il CVC vuole 3 o 4 cifre",
        ));
    }
    rif.insert("brand".into(), Value::from(marca(&numero)));
    rif.insert("last4".into(), Value::from(&numero[numero.len() - 4..]));
    cam.insert("numero".into(), Value::from(numero));
    if let Some(m) = rif.get("exp_month") {
        let m = intero(m).filter(|m| (1..=12).contains(m)).ok_or_else(|| {
            Errore::nuovo(Codice::CartaNonValida, "exp_month vuole un mese da 1 a 12")
        })?;
        rif.insert("exp_month".into(), Value::from(m));
    }
    if let Some(a) = rif.get("exp_year") {
        let a = intero(a)
            .map(|a| if a < 100 { a + 2000 } else { a })
            .filter(|a| (2000..=2100).contains(a))
            .ok_or_else(|| Errore::nuovo(Codice::CartaNonValida, "exp_year non valido"))?;
        rif.insert("exp_year".into(), Value::from(a));
    }
    Ok(())
}

fn intero(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Il controllo di Luhn su una stringa di sole cifre.
pub fn luhn(cifre: &str) -> bool {
    if cifre.is_empty() || !cifre.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let somma: u32 = cifre
        .chars()
        .rev()
        .filter_map(|c| c.to_digit(10))
        .enumerate()
        .map(|(i, d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                d
            }
        })
        .sum();
    somma.is_multiple_of(10)
}

/// La marca della carta dal numero (prefissi IIN).
pub fn marca(numero: &str) -> &'static str {
    let p = |n: usize| {
        numero
            .get(..n)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0)
    };
    match () {
        () if matches!(p(2), 34 | 37) => "amex",
        () if numero.starts_with('4') => "visa",
        () if (51..=55).contains(&p(2)) || (2221..=2720).contains(&p(4)) => "mastercard",
        () if p(4) == 6011 || p(2) == 65 || (644..=649).contains(&p(3)) => "discover",
        () if p(2) == 62 => "unionpay",
        () if (3528..=3589).contains(&p(4)) => "jcb",
        () if (300..=305).contains(&p(3)) || matches!(p(2), 36 | 38) => "diners",
        () if p(2) == 50 || (56..=69).contains(&p(2)) => "maestro",
        () => "sconosciuta",
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json::json;

    fn m(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn luhn_e_marca() {
        assert!(luhn("4111111111111111"));
        assert!(luhn("5555555555554444"));
        assert!(luhn("378282246310005"));
        assert!(!luhn("4111111111111112"));
        assert!(!luhn("41111a1111111111"));
        assert!(!luhn(""));
        assert_eq!(marca("4111111111111111"), "visa");
        assert_eq!(marca("5555555555554444"), "mastercard");
        assert_eq!(marca("2223003122003222"), "mastercard");
        assert_eq!(marca("378282246310005"), "amex");
        assert_eq!(marca("6011111111111117"), "discover");
        assert_eq!(marca("3530111333300000"), "jcb");
        assert_eq!(marca("9999999999999995"), "sconosciuta");
    }

    #[test]
    fn carta_ricava_brand_e_last4() {
        let (rif, cam) = normalizza(
            GenereCassaforte::Carta,
            &m(json!({"brand": "finta", "exp_month": "7", "exp_year": 29, "intestatario": "Mario"})),
            &m(json!({"numero": "4111 1111-1111 1111", "cvc": "123"})),
        )
        .unwrap();
        assert_eq!(rif["brand"], "visa");
        assert_eq!(rif["last4"], "1111");
        assert_eq!(rif["exp_month"], 7);
        assert_eq!(rif["exp_year"], 2029);
        assert_eq!(cam["numero"], "4111111111111111");
        let e = normalizza(
            GenereCassaforte::Carta,
            &Map::new(),
            &m(json!({"numero": "4111111111111112"})),
        )
        .unwrap_err();
        assert_eq!(e.codice, Codice::CartaNonValida);
    }

    #[test]
    fn regole_degli_altri_generi() {
        let ok = normalizza(
            GenereCassaforte::Accesso,
            &m(json!({"utente": "mario"})),
            &m(json!({"password": "x", "passphrase": "frase", "pin": 1234})),
        )
        .unwrap();
        assert_eq!(ok.1["pin"], "1234");
        assert!(normalizza(GenereCassaforte::Accesso, &Map::new(), &Map::new()).is_err());
        assert!(
            normalizza(
                GenereCassaforte::Accesso,
                &m(json!({"sconosciuto": 1})),
                &m(json!({"password": "x"}))
            )
            .is_err()
        );
        assert!(
            normalizza(
                GenereCassaforte::Contatto,
                &m(json!({"nome": "Anna"})),
                &m(json!({"password": "x"}))
            )
            .is_err()
        );
        assert!(
            normalizza(
                GenereCassaforte::Segreto,
                &m(json!({"variabile": "minuscola"})),
                &m(json!({"valore": "v"}))
            )
            .is_err()
        );
        assert!(
            normalizza(
                GenereCassaforte::Segreto,
                &m(json!({"variabile": "GITHUB_TOKEN"})),
                &m(json!({"valore": "v"}))
            )
            .is_ok()
        );
        assert!(siti(&vec!["a".to_string(); 21]).is_err());
        assert_eq!(
            siti(&[" https://x ".into(), "".into()]).unwrap(),
            ["https://x"]
        );
    }
}
