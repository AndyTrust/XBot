//! L'anteprima della barra: l'ultima riga `messaggio` di ogni assistente, in testo semplice.
//! Una query a parte, accanto alla vista `v_barra_laterale` (che non cambia).

use std::collections::HashMap;

use rusqlite::Connection;
use serde_json::Value;
use xbot_core::{Ms, Risposta};

use crate::util::RisultatoDb;

/// Lunghezza massima dell'anteprima, in caratteri (con i puntini).
pub const ANTEPRIMA: usize = 120;

/// assistente → (anteprima, creata_ms) dell'ultima riga `messaggio` non soppressa delle sue
/// sessioni, di qualsiasi ruolo.
pub fn anteprime(conn: &Connection) -> Risposta<HashMap<String, (String, Ms)>> {
    let mut st = conn
        .prepare(
            "SELECT assistente_id, corpo_json, creata_ms FROM (
               SELECT s.assistente_id, r.corpo_json, r.creata_ms,
                 ROW_NUMBER() OVER (PARTITION BY s.assistente_id
                                    ORDER BY r.creata_ms DESC, r.seq DESC) AS n
               FROM riga r JOIN sessione s ON s.id = r.sessione_id
               WHERE r.genere = 'messaggio' AND r.soppressa = 0)
             WHERE n = 1",
        )
        .db()?;
    let righe = st
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Ms>(2)?,
            ))
        })
        .db()?
        .collect::<rusqlite::Result<Vec<_>>>()
        .db()?;
    Ok(righe
        .into_iter()
        .map(|(a, corpo, ms)| {
            let testo = corpo
                .and_then(|c| serde_json::from_str::<Value>(&c).ok())
                .and_then(|v| v["testo"].as_str().map(str::to_string))
                .unwrap_or_default();
            (a, (anteprima(&testo), ms))
        })
        .collect())
}

/// Il testo senza Markdown (titoli, enfasi, codice, link, citazioni, elenchi), su una riga,
/// tagliato a `ANTEPRIMA` caratteri.
pub fn anteprima(testo: &str) -> String {
    let mut righe = Vec::new();
    for riga in testo.lines() {
        let r = riga.trim();
        if r.starts_with("```") || r.starts_with("~~~") {
            continue;
        }
        let r = r.trim_start_matches(['#', '>', ' ']);
        let r = r
            .strip_prefix("- ")
            .or_else(|| r.strip_prefix("* "))
            .or_else(|| r.strip_prefix("+ "))
            .unwrap_or(r);
        righe.push(senza_link(r));
    }
    let piatto: String = righe
        .join(" ")
        .chars()
        .filter(|c| !matches!(c, '*' | '_' | '`' | '~'))
        .collect();
    let semplice = piatto.split_whitespace().collect::<Vec<_>>().join(" ");
    if semplice.chars().count() <= ANTEPRIMA {
        return semplice;
    }
    let mut corto: String = semplice.chars().take(ANTEPRIMA - 1).collect();
    corto = corto.trim_end().to_string();
    corto.push('…');
    corto
}

/// `[testo](url)` e `![alt](url)` → `testo` / `alt`.
fn senza_link(r: &str) -> String {
    let mut uscita = String::new();
    let mut resto = r;
    while let Some(i) = resto.find('[') {
        let (prima, dopo) = resto.split_at(i);
        let prima = prima.strip_suffix('!').unwrap_or(prima);
        match dopo
            .find("](")
            .and_then(|j| dopo[j..].find(')').map(|k| (j, j + k)))
        {
            Some((j, k)) => {
                uscita.push_str(prima);
                uscita.push_str(&dopo[1..j]);
                resto = &dopo[k + 1..];
            }
            None => {
                uscita.push_str(prima);
                uscita.push('[');
                resto = &dopo[1..];
            }
        }
    }
    uscita.push_str(resto);
    uscita
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn markdown_tolto() {
        let t = "# Titolo\n\n**Ciao** _Mario_, vedi [il sito](https://x.it) e `codice`.\n```rust\nfn x() {}\n```\n- punto\n> citazione";
        assert_eq!(
            anteprima(t),
            "Titolo Ciao Mario, vedi il sito e codice. fn x() {} punto citazione"
        );
        assert_eq!(anteprima("![logo](a.png) [aperta"), "logo [aperta");
    }

    #[test]
    fn tagliata_a_120() {
        let a = anteprima(&"è".repeat(300));
        assert_eq!(a.chars().count(), ANTEPRIMA);
        assert!(a.ends_with('…'));
        assert_eq!(anteprima("corto"), "corto");
    }
}
