//! `GET /utilizzo`: token e costo dei turni, per provider, assistente, modello e giorno.

use rusqlite::{Connection, params};
use serde_json::{Value, json};
use xbot_core::Risposta;

use crate::util::tutte_json;

const SOMME: &str = "COALESCE(SUM(t.token_ingresso), 0) AS token_ingresso,
    COALESCE(SUM(t.token_uscita), 0) AS token_uscita,
    COALESCE(SUM(t.token_cache), 0) AS token_cache,
    COALESCE(SUM(t.costo_usd), 0.0) AS costo_usd, COUNT(*) AS turni";

const DA: &str = "FROM turno t LEFT JOIN sessione s ON s.id = t.sessione_id
    LEFT JOIN assistente a ON a.id = s.assistente_id
    WHERE t.iniziato_ms >= ?1 AND t.iniziato_ms < ?2
      AND (?3 IS NULL OR s.assistente_id = ?3) AND (?4 IS NULL OR t.provider_id = ?4)
      AND (?5 IS NULL OR t.modello_id = ?5)";

/// I filtri facoltativi di `GET /utilizzo`: valgono per tutte le tabelle della risposta.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct Filtri {
    pub assistente_id: Option<String>,
    pub provider_id: Option<String>,
    pub modello_id: Option<String>,
}

/// L'utilizzo fra `da_ms` (compreso) e `a_ms` (escluso). Il giorno è quello dell'ora locale
/// della macchina del server.
pub fn riepilogo(conn: &Connection, da_ms: i64, a_ms: i64, f: &Filtri) -> Risposta<Value> {
    let vuoto = |x: &Option<String>| x.clone().filter(|v| !v.is_empty());
    let (fa, fp, fm) = (
        vuoto(&f.assistente_id),
        vuoto(&f.provider_id),
        vuoto(&f.modello_id),
    );
    let p = params![da_ms, a_ms, fa, fp, fm];
    let q = |gruppo: &str, colonne: &str| {
        tutte_json(
            conn,
            &format!(
                "SELECT {colonne}, {SOMME} {DA} GROUP BY {gruppo} ORDER BY costo_usd DESC, turni DESC"
            ),
            p,
        )
    };
    Ok(json!({
        "da_ms": da_ms,
        "a_ms": a_ms,
        "filtri": { "assistente_id": fa, "provider_id": fp, "modello_id": fm },
        "per_provider": q("t.provider_id", "t.provider_id")?,
        "per_assistente": q("s.assistente_id", "s.assistente_id, a.nome")?,
        "per_modello": q("t.modello_id", "t.modello_id, t.provider_id")?,
        "per_giorno": tutte_json(
            conn,
            &format!(
                "SELECT date(t.iniziato_ms / 1000, 'unixepoch', 'localtime') AS giorno,
                   COALESCE(SUM(t.token_ingresso), 0) + COALESCE(SUM(t.token_uscita), 0) AS token,
                   COALESCE(SUM(t.costo_usd), 0.0) AS costo_usd, COUNT(*) AS turni
                 {DA} GROUP BY giorno ORDER BY giorno"
            ),
            p,
        )?,
    }))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn vuoto_su_database_nuovo() {
        let db = crate::db_prova();
        db.con(|c| {
            let v = riepilogo(c, 0, i64::MAX, &Filtri::default())?;
            assert!(v["per_provider"].as_array().unwrap().is_empty());
            assert!(v["per_giorno"].as_array().unwrap().is_empty());
            Ok(())
        })
        .unwrap();
    }
}
