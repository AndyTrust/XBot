//! I collegamenti «vivi»: con intestazioni e ambiente decifrati, per la prova e per il turno.

use rusqlite::{Connection, OptionalExtension};
use xbot_core::{Codice, Errore, Risposta, SegretoTesto};

use super::{CollegamentoVivo, decifra_mappa};
use crate::cassaforte::Chiave;
use crate::util::RisultatoDb;

pub(super) fn vivo(
    conn: &Connection,
    chiave: Option<&Chiave>,
    id: &str,
) -> Risposta<CollegamentoVivo> {
    type Riga = (
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        bool,
        Option<String>,
    );
    let (nome, trasporto, comando, arg, url, intest, amb, richiede, macchina_id): Riga = conn
        .query_row(
            "SELECT nome, trasporto, comando, argomenti_json, url, intestazioni_cifrate, ambiente_cifrato,
               richiede_oauth, macchina_id FROM collegamento WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get::<_, i64>(7)? == 1,
                    r.get(8)?,
                ))
            },
        )
        .optional()
        .db()?
        .ok_or_else(|| Errore::nuovo(Codice::NonTrovato, "collegamento non trovato"))?;
    let mut intestazioni = decifra_mappa(chiave, id, "intestazioni", intest)?;
    // --- 2.I --- l'accesso OAuth `ok` vince su un'`Authorization` scritta a mano.
    let accesso = crate::oauth::leggi(conn, chiave, id)?;
    let oauth = accesso.as_ref().map(|a| a.stato.clone());
    if let Some(a) = accesso
        && a.stato == "ok"
        && let Some(t) = a.token
    {
        intestazioni.retain(|(k, _)| !k.eq_ignore_ascii_case("authorization"));
        intestazioni.push((
            "Authorization".into(),
            SegretoTesto::nuovo(format!("Bearer {}", t.access_token)),
        ));
    }
    Ok(CollegamentoVivo {
        id: id.to_string(),
        nome,
        trasporto,
        comando,
        argomenti: serde_json::from_str(&arg).unwrap_or_default(),
        url,
        intestazioni,
        ambiente: decifra_mappa(chiave, id, "ambiente", amb)?,
        richiede_oauth: richiede || oauth.is_some(),
        oauth,
        macchina_id,
    })
}

/// Un collegamento con i segreti decifrati (per la prova).
pub fn per_prova(
    conn: &Connection,
    chiave: Option<&Chiave>,
    id: &str,
) -> Risposta<CollegamentoVivo> {
    vivo(conn, chiave, id)
}

/// I collegamenti accesi per un assistente (2.L: tutti, salvo quelli che lo escludono), con i segreti decifrati (per il turno). Quelli che
/// vogliono OAuth senza un accesso `ok` restano fuori (2.I).
/// Uno che non si decifra si salta con un avviso nel log: il turno parte lo stesso.
pub fn per_assistente(
    conn: &Connection,
    chiave: Option<&Chiave>,
    assistente_id: &str,
) -> Risposta<Vec<CollegamentoVivo>> {
    let ids: Vec<String> = {
        let mut st = conn
            .prepare(&format!(
                "SELECT c.id FROM collegamento c WHERE c.abilitato = 1 AND {}
                     ORDER BY c.nome, c.id",
                super::NON_ESCLUSO
            ))
            .db()?;
        st.query_map([assistente_id], |r| r.get(0))
            .db()?
            .collect::<rusqlite::Result<_>>()
            .db()?
    };
    let mut v = Vec::new();
    for id in ids {
        match vivo(conn, chiave, &id) {
            Ok(c) if c.pronto() => v.push(c),
            Ok(_) => {
                tracing::info!(collegamento = %id, "collegamento senza accesso OAuth valido: fuori dal turno")
            }
            Err(e) => {
                tracing::warn!(collegamento = %id, codice = e.codice.come_testo(), "collegamento saltato nel turno")
            }
        }
    }
    Ok(v)
}
