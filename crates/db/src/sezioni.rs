//! Sezioni della barra laterale e la vista `v_barra_laterale`.

use rusqlite::{Connection, params};
use serde::Deserialize;
use serde_json::{Map, Value};
use xbot_core::{Errore, Risposta};

use crate::util::{RisultatoDb, aggiorna_campi, in_transazione, tutte_json};

pub fn elenca(conn: &Connection) -> Risposta<Vec<Value>> {
    tutte_json(
        conn,
        "SELECT id, nome, ordine, chiusa FROM sezione_barra ORDER BY ordine, nome",
        [],
    )
}

pub fn crea(conn: &Connection, id: &str, nome: &str, ordine: i64) -> Risposta<Value> {
    if nome.trim().is_empty() {
        return Err(Errore::non_valida("il nome della sezione è obbligatorio"));
    }
    crate::controlli::testo_valido("nome", Some(nome), true)?;
    crate::controlli::lunghezza("nome", Some(nome), crate::controlli::NOME_ASSISTENTE)?;
    conn.execute(
        "INSERT INTO sezione_barra (id, nome, ordine) VALUES (?1, ?2, ?3)",
        params![id, nome.trim(), ordine],
    )
    .db()?;
    Ok(serde_json::json!({ "id": id, "nome": nome.trim(), "ordine": ordine, "chiusa": 0 }))
}

/// Una modifica di sezione dentro `PATCH /sezioni` (nome, ordine, chiusa).
#[derive(Debug, Clone, Deserialize)]
pub struct ModificaSezione {
    pub id: String,
    #[serde(flatten)]
    pub campi: Map<String, Value>,
}

/// Modifica più sezioni insieme (riordino della barra).
pub fn aggiorna_molte(conn: &Connection, modifiche: &[ModificaSezione]) -> Risposta<Vec<Value>> {
    in_transazione(conn, || {
        for m in modifiche {
            crate::controlli::nomi_validi(&m.campi, &["nome"])?;
            let n = aggiorna_campi(
                conn,
                "sezione_barra",
                "id",
                &m.id,
                &m.campi,
                &["nome", "ordine", "chiusa"],
                &[],
            )?;
            if n == 0 && !m.campi.is_empty() {
                return Err(Errore::nuovo(
                    xbot_core::Codice::SezioneNonTrovata,
                    format!("sezione non trovata: {}", m.id),
                ));
            }
        }
        elenca(conn)
    })
}

/// La barra: tutte le sezioni in ordine, ognuna con i suoi assistenti dalla vista,
/// più `anteprima` e `anteprima_ms` (l'ultima riga `messaggio`, da `barra::anteprime`).
pub fn barra(conn: &Connection) -> Risposta<Vec<Value>> {
    let sezioni = elenca(conn)?;
    let voci = tutte_json(
        conn,
        "SELECT v.*, sa.ordine AS ordine_in_sezione FROM v_barra_laterale v
         JOIN sezione_assistente sa ON sa.assistente_id = v.assistente_id AND sa.sezione_id = v.sezione_id
         ORDER BY v.fissato DESC, sa.ordine, v.nome",
        [],
    )?;
    let anteprime = crate::barra::anteprime(conn)?;
    let voci: Vec<Value> = voci
        .into_iter()
        .map(|mut v| {
            let a = v["assistente_id"].as_str().and_then(|id| anteprime.get(id));
            if let Value::Object(m) = &mut v {
                m.insert(
                    "anteprima".into(),
                    a.map_or(Value::Null, |a| Value::from(a.0.clone())),
                );
                m.insert(
                    "anteprima_ms".into(),
                    a.map_or(Value::Null, |a| Value::from(a.1)),
                );
            }
            v
        })
        .collect();
    let mut uscita = Vec::new();
    for mut s in sezioni {
        let id = s["id"].clone();
        let assistenti: Vec<Value> = voci
            .iter()
            .filter(|v| v["sezione_id"] == id)
            .cloned()
            .collect();
        if let Value::Object(m) = &mut s {
            m.insert("assistenti".into(), Value::Array(assistenti));
        }
        uscita.push(s);
    }
    Ok(uscita)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::assistenti::{self, NuovoAssistente};
    use crate::sessioni::{self, NuovaSessione};
    use serde_json::json;
    use xbot_core::{GenereRiga, Ruolo};

    #[test]
    fn vista_barra_laterale_conta_non_letti() {
        let db = crate::db_prova();
        db.con(|c| {
            let a = assistenti::crea(
                c,
                &NuovoAssistente {
                    nome: "Atena".into(),
                    ..Default::default()
                },
            )?;
            let aid = a["id"].as_str().unwrap();
            let s = sessioni::crea(c, aid, &NuovaSessione::default())?;
            crate::righe::aggiungi(
                c,
                &s.id,
                GenereRiga::Messaggio,
                Some(Ruolo::Utente),
                &json!({}),
            )?;
            crate::righe::aggiungi(
                c,
                &s.id,
                GenereRiga::Messaggio,
                Some(Ruolo::Assistente),
                &json!({}),
            )?;
            crate::righe::aggiungi(
                c,
                &s.id,
                GenereRiga::Messaggio,
                Some(Ruolo::Assistente),
                &json!({}),
            )?;
            let b = barra(c)?;
            let non_assegnati = b.iter().find(|s| s["id"] == "__non_assegnati__").unwrap();
            let voce = &non_assegnati["assistenti"][0];
            assert_eq!(voce["nome"], "Atena");
            assert_eq!(voce["non_letti"], 2);
            // Dalla 002 i non letti contano per tempo: dopo «letto» sono zero...
            crate::lettura::segna_letto(c, aid, 2)?;
            let non_letti = |c: &rusqlite::Connection| {
                let b = barra(c).unwrap();
                b.iter().find(|s| s["id"] == "__non_assegnati__").unwrap()["assistenti"][0]
                    ["non_letti"]
                    .clone()
            };
            assert_eq!(non_letti(c), 0);
            // ...e un messaggio nuovo dell'assistente, in un'altra sessione, torna a contare.
            std::thread::sleep(std::time::Duration::from_millis(3));
            let s2 = sessioni::crea(c, aid, &NuovaSessione::default())?;
            let r = crate::righe::aggiungi(
                c,
                &s2.id,
                GenereRiga::Messaggio,
                Some(Ruolo::Assistente),
                &json!({}),
            )?;
            assert_eq!(non_letti(c), 1);
            c.execute("UPDATE riga SET soppressa = 1 WHERE id = ?1", [&r.id])
                .unwrap();
            assert_eq!(non_letti(c), 0, "le righe soppresse non contano");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn riordino_sezioni() {
        let db = crate::db_prova();
        db.con(|c| {
            crea(c, "a", "A", 5)?;
            crea(c, "b", "B", 6)?;
            let m: Vec<ModificaSezione> =
                serde_json::from_value(json!([{"id": "b", "ordine": 1}, {"id": "a", "chiusa": 1}]))
                    .unwrap();
            let v = aggiorna_molte(c, &m)?;
            assert_eq!(v[0]["id"], "b");
            assert!(
                aggiorna_molte(
                    c,
                    &serde_json::from_value::<Vec<ModificaSezione>>(
                        json!([{"id": "zz", "ordine": 1}])
                    )
                    .unwrap()
                )
                .is_err()
            );
            Ok(())
        })
        .unwrap();
    }
}
