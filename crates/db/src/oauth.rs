//! Gli accessi OAuth dei collegamenti (Fase 2.I): un account per collegamento, token cifrati con
//! la chiave della cassaforte. Dalle rotte esce solo `pubblico()`: mai token né `client_secret`.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use xbot_core::{Codice, Errore, Ms, Risposta, SegretoTesto, nuovo_id, ora_ms};

use crate::cassaforte::{self, Chiave};
use crate::util::RisultatoDb;

/// I token come li salva XBot (il JSON cifrato in `token_cifrato`).
#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct Token {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default = "bearer")]
    pub token_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<Ms>,
}

fn bearer() -> String {
    "bearer".into()
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Token")
            .field("refresh", &self.refresh_token.is_some())
            .field("expires_at_ms", &self.expires_at_ms)
            .finish_non_exhaustive()
    }
}

/// Un accesso completo, con i segreti decifrati (solo dentro il server).
#[derive(Debug, Clone)]
pub struct Accesso {
    pub collegamento_id: String,
    pub account: String,
    pub stato: String,
    pub token: Option<Token>,
    pub client_id: Option<String>,
    pub client_secret: Option<SegretoTesto>,
    pub issuer: Option<String>,
    pub scopi: Option<String>,
    pub scade_ms: Option<Ms>,
}

fn aad(collegamento_id: &str, campo: &str) -> String {
    format!("oauth:{collegamento_id}:{campo}")
}

fn chiave_o_errore(chiave: Option<&Chiave>) -> Risposta<&Chiave> {
    chiave.ok_or_else(|| {
        Errore::nuovo(
            Codice::CassaforteChiusa,
            "la cassaforte è chiusa: i token OAuth non si possono cifrare",
        )
    })
}

/// Salva (o sostituisce) l'accesso di un collegamento, in stato `ok`.
pub fn salva(conn: &Connection, chiave: Option<&Chiave>, a: &Accesso) -> Risposta<()> {
    let k = chiave_o_errore(chiave)?;
    let token = match &a.token {
        Some(t) => {
            let chiaro = SegretoTesto::nuovo(json!(t).to_string());
            Some(cassaforte::cifra(
                k,
                &aad(&a.collegamento_id, "token"),
                chiaro.esponi().as_bytes(),
            )?)
        }
        None => None,
    };
    let segreto = match &a.client_secret {
        Some(s) => Some(cassaforte::cifra(
            k,
            &aad(&a.collegamento_id, "client_secret"),
            s.esponi().as_bytes(),
        )?),
        None => None,
    };
    let scade = a.token.as_ref().and_then(|t| t.expires_at_ms);
    conn.execute(
        "INSERT INTO oauth_account (id, collegamento_id, account, token_cifrato, scade_ms,
           aggiornato_ms, client_id, client_secret_cifrato, issuer, scopi, stato)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(collegamento_id) DO UPDATE SET account = excluded.account,
           token_cifrato = excluded.token_cifrato, scade_ms = excluded.scade_ms,
           aggiornato_ms = excluded.aggiornato_ms, client_id = excluded.client_id,
           client_secret_cifrato = excluded.client_secret_cifrato, issuer = excluded.issuer,
           scopi = excluded.scopi, stato = excluded.stato",
        params![
            nuovo_id(),
            a.collegamento_id,
            a.account,
            token,
            scade,
            ora_ms(),
            a.client_id,
            segreto,
            a.issuer,
            a.scopi,
            a.stato
        ],
    )
    .db()?;
    conn.execute(
        "UPDATE collegamento SET richiede_oauth = 1, ultimo_errore = NULL WHERE id = ?1",
        [&a.collegamento_id],
    )
    .db()?;
    Ok(())
}

/// Solo i token nuovi dopo un rinnovo (stato `ok`).
pub fn aggiorna_token(
    conn: &Connection,
    chiave: Option<&Chiave>,
    collegamento_id: &str,
    t: &Token,
) -> Risposta<()> {
    let k = chiave_o_errore(chiave)?;
    let chiaro = SegretoTesto::nuovo(json!(t).to_string());
    let blob = cassaforte::cifra(
        k,
        &aad(collegamento_id, "token"),
        chiaro.esponi().as_bytes(),
    )?;
    conn.execute(
        "UPDATE oauth_account SET token_cifrato = ?2, scade_ms = ?3, aggiornato_ms = ?4, stato = 'ok'
         WHERE collegamento_id = ?1",
        params![collegamento_id, blob, t.expires_at_ms, ora_ms()],
    )
    .db()?;
    Ok(())
}

/// L'accesso di un collegamento, con i segreti decifrati. `None` se non c'è una riga.
pub fn leggi(
    conn: &Connection,
    chiave: Option<&Chiave>,
    collegamento_id: &str,
) -> Risposta<Option<Accesso>> {
    type Riga = (
        String,
        String,
        Option<Vec<u8>>,
        Option<Ms>,
        Option<String>,
        Option<Vec<u8>>,
        Option<String>,
        Option<String>,
    );
    let riga: Option<Riga> = conn
        .query_row(
            "SELECT account, stato, token_cifrato, scade_ms, client_id, client_secret_cifrato,
               issuer, scopi FROM oauth_account WHERE collegamento_id = ?1",
            [collegamento_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            },
        )
        .optional()
        .db()?;
    let Some((account, stato, tok, scade_ms, client_id, segr, issuer, scopi)) = riga else {
        return Ok(None);
    };
    let decifra = |campo: &str, blob: &[u8]| -> Risposta<SegretoTesto> {
        let k = chiave_o_errore(chiave)?;
        let b = cassaforte::decifra(k, &aad(collegamento_id, campo), blob)?;
        String::from_utf8(b)
            .map(SegretoTesto::from)
            .map_err(|_| Errore::nuovo(Codice::Cifratura, "token non in UTF-8"))
    };
    let token = match tok {
        Some(b) => {
            let s = decifra("token", &b)?;
            Some(
                serde_json::from_str::<Token>(s.esponi())
                    .map_err(|_| Errore::nuovo(Codice::Cifratura, "token illeggibili"))?,
            )
        }
        None => None,
    };
    let client_secret = match segr {
        Some(b) => Some(decifra("client_secret", &b)?),
        None => None,
    };
    Ok(Some(Accesso {
        collegamento_id: collegamento_id.to_string(),
        account,
        stato,
        token,
        client_id,
        client_secret,
        issuer,
        scopi,
        scade_ms,
    }))
}

/// Cambia lo stato (`scaduto`); con `revocato` cancella anche i token.
pub fn segna(conn: &Connection, collegamento_id: &str, stato: &str) -> Risposta<()> {
    if stato == "revocato" {
        conn.execute(
            "UPDATE oauth_account SET stato = 'revocato', token_cifrato = NULL, scade_ms = NULL,
               aggiornato_ms = ?2 WHERE collegamento_id = ?1",
            params![collegamento_id, ora_ms()],
        )
        .db()?;
        conn.execute(
            "UPDATE collegamento SET abilitato = 0 WHERE id = ?1",
            [collegamento_id],
        )
        .db()?;
    } else {
        conn.execute(
            "UPDATE oauth_account SET stato = ?2, aggiornato_ms = ?3 WHERE collegamento_id = ?1",
            params![collegamento_id, stato, ora_ms()],
        )
        .db()?;
    }
    Ok(())
}

/// Lo stato visibile: `{stato, account, scade_ms, aggiornato_ms, issuer}` o `null`.
pub fn pubblico(conn: &Connection, collegamento_id: &str) -> Risposta<Value> {
    let v = crate::util::una_json(
        conn,
        "SELECT stato, account, scade_ms, aggiornato_ms, issuer FROM oauth_account
          WHERE collegamento_id = ?1",
        [collegamento_id],
    )?;
    Ok(v.unwrap_or(Value::Null))
}

/// Il nome di un collegamento (per eventi e notifiche).
pub fn nome_collegamento(conn: &Connection, id: &str) -> Risposta<String> {
    conn.query_row("SELECT nome FROM collegamento WHERE id = ?1", [id], |r| {
        r.get(0)
    })
    .optional()
    .db()?
    .ok_or_else(|| Errore::nuovo(Codice::NonTrovato, "collegamento non trovato"))
}

/// Gli assistenti che usano un collegamento e hanno una routine attiva (per l'avviso «tocca a te»).
pub fn assistenti_con_routine(conn: &Connection, collegamento_id: &str) -> Risposta<Vec<String>> {
    let mut st = conn
        .prepare(
            "SELECT DISTINCT r.assistente_id FROM routine r
             WHERE r.abilitata = 1 AND NOT EXISTS (SELECT 1 FROM collegamento_assistente ca
                WHERE ca.collegamento_id = ?1 AND ca.assistente_id = r.assistente_id
                  AND ca.abilitato = 1)",
        )
        .db()?;
    st.query_map([collegamento_id], |r| r.get(0))
        .db()?
        .collect::<rusqlite::Result<_>>()
        .db()
}

/// Un client registrato a mano (server senza registrazione dinamica, es. Slack): il prossimo
/// `avvia` lo usa. Senza un accesso precedente la riga nasce `revocato`, senza token.
pub fn imposta_client(
    conn: &Connection,
    chiave: Option<&Chiave>,
    collegamento_id: &str,
    client_id: &str,
    client_secret: Option<&SegretoTesto>,
) -> Risposta<()> {
    if client_id.trim().is_empty() || client_id.len() > 500 {
        return Err(Errore::non_valida("client_id vuoto o troppo lungo"));
    }
    let segreto = match client_secret {
        Some(s) => Some(cassaforte::cifra(
            chiave_o_errore(chiave)?,
            &aad(collegamento_id, "client_secret"),
            s.esponi().as_bytes(),
        )?),
        None => None,
    };
    let host = nome_collegamento(conn, collegamento_id)?;
    conn.execute(
        "INSERT INTO oauth_account (id, collegamento_id, account, aggiornato_ms, client_id,
           client_secret_cifrato, stato) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'revocato')
         ON CONFLICT(collegamento_id) DO UPDATE SET client_id = excluded.client_id,
           client_secret_cifrato = excluded.client_secret_cifrato,
           aggiornato_ms = excluded.aggiornato_ms",
        params![
            nuovo_id(),
            collegamento_id,
            host,
            ora_ms(),
            client_id.trim(),
            segreto
        ],
    )
    .db()?;
    segna_richiede(conn, collegamento_id, None)
}

/// Segna che il collegamento vuole OAuth (dalla scoperta) e, se c'è, l'errore da mostrare.
pub fn segna_richiede(conn: &Connection, id: &str, errore: Option<&str>) -> Risposta<()> {
    conn.execute(
        "UPDATE collegamento SET richiede_oauth = 1, ultimo_errore = COALESCE(?2, ultimo_errore)
          WHERE id = ?1",
        params![id, errore],
    )
    .db()?;
    Ok(())
}

/// Gli accessi `ok` dei collegamenti accesi di un assistente (da rinnovare prima del turno).
pub fn ok_per_assistente(conn: &Connection, assistente_id: &str) -> Risposta<Vec<String>> {
    let mut st = conn
        .prepare(
            "SELECT o.collegamento_id FROM oauth_account o
              JOIN collegamento c ON c.id = o.collegamento_id
             WHERE c.abilitato = 1 AND o.stato = 'ok' AND NOT EXISTS (SELECT 1
                FROM collegamento_assistente ca WHERE ca.collegamento_id = c.id
                 AND ca.assistente_id = ?1 AND ca.abilitato = 1) ORDER BY c.nome",
        )
        .db()?;
    st.query_map([assistente_id], |r| r.get(0))
        .db()?
        .collect::<rusqlite::Result<_>>()
        .db()
}

/// I collegamenti accesi di un assistente che vogliono OAuth e non hanno un accesso `ok`.
pub fn senza_accesso_per_assistente(
    conn: &Connection,
    assistente_id: &str,
) -> Risposta<Vec<String>> {
    let mut st = conn
        .prepare(
            "SELECT c.nome FROM collegamento c
              LEFT JOIN oauth_account o ON o.collegamento_id = c.id
             WHERE c.abilitato = 1 AND NOT EXISTS (SELECT 1 FROM collegamento_assistente ca
                WHERE ca.collegamento_id = c.id AND ca.assistente_id = ?1 AND ca.abilitato = 1)
               AND (c.richiede_oauth = 1 OR o.id IS NOT NULL) AND COALESCE(o.stato, '') != 'ok'
             ORDER BY c.nome",
        )
        .db()?;
    st.query_map([assistente_id], |r| r.get(0))
        .db()?
        .collect::<rusqlite::Result<_>>()
        .db()
}

/// Lo scrive anche come `ultimo_errore` del collegamento.
pub fn segna_scaduto(conn: &Connection, id: &str, errore: &str) -> Risposta<()> {
    segna(conn, id, "scaduto")?;
    conn.execute(
        "UPDATE collegamento SET ultimo_errore = ?2 WHERE id = ?1",
        params![id, errore],
    )
    .db()?;
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    fn collegamento(c: &Connection) -> String {
        let n = crate::collegamenti::NuovoCollegamento {
            nome: "Granola".into(),
            trasporto: "http".into(),
            url: Some("https://mcp.granola.ai/mcp".into()),
            ..Default::default()
        };
        crate::collegamenti::crea(c, None, &n).unwrap()
    }

    #[test]
    fn salva_leggi_rinnova_revoca() {
        let db = crate::db_prova();
        let k = Chiave::da_byte([7u8; 32]);
        db.con(|c| {
            let id = collegamento(c);
            assert!(leggi(c, Some(&k), &id)?.is_none());
            assert_eq!(pubblico(c, &id)?, Value::Null);
            let t = Token {
                access_token: "AT-segreto".into(),
                refresh_token: Some("RT-segreto".into()),
                token_type: "bearer".into(),
                scope: None,
                expires_at_ms: Some(123),
            };
            let a = Accesso {
                collegamento_id: id.clone(),
                account: "utente@prova".into(),
                stato: "ok".into(),
                token: Some(t.clone()),
                client_id: Some("cid".into()),
                client_secret: Some(SegretoTesto::nuovo("CS-segreto")),
                issuer: Some("https://auth".into()),
                scopi: None,
                scade_ms: None,
            };
            assert!(salva(c, None, &a).is_err(), "senza chiave non si salva");
            salva(c, Some(&k), &a)?;
            let l = leggi(c, Some(&k), &id)?.unwrap();
            assert_eq!(l.token.as_ref(), Some(&t));
            assert_eq!(l.client_secret.unwrap().esponi(), "CS-segreto");
            let p = pubblico(c, &id)?;
            assert_eq!(p["account"], "utente@prova");
            assert_eq!(p["scade_ms"], 123);
            assert!(!p.to_string().contains("segreto"));
            let t2 = Token {
                access_token: "AT2".into(),
                expires_at_ms: Some(999),
                ..t
            };
            aggiorna_token(c, Some(&k), &id, &t2)?;
            assert_eq!(
                leggi(c, Some(&k), &id)?
                    .unwrap()
                    .token
                    .unwrap()
                    .access_token,
                "AT2"
            );
            segna(c, &id, "scaduto")?;
            assert_eq!(pubblico(c, &id)?["stato"], "scaduto");
            segna(c, &id, "revocato")?;
            let l = leggi(c, Some(&k), &id)?.unwrap();
            assert!(l.token.is_none());
            assert_eq!(l.client_id.as_deref(), Some("cid"));
            let tok: Option<Vec<u8>> = c
                .query_row(
                    "SELECT token_cifrato FROM oauth_account WHERE collegamento_id = ?1",
                    [&id],
                    |r| r.get(0),
                )
                .db()?;
            assert!(tok.is_none(), "token cancellati, non solo marcati");
            Ok(())
        })
        .unwrap();
    }
}
