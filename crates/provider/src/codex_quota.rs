//! Fase 2.I: la quota dell'abbonamento ChatGPT come la scrive Codex. Il client non la stampa in
//! `exec --json`, ma la salva nel file della sessione (`$CODEX_HOME/sessions/AAAA/MM/GG/
//! rollout-…-<thread_id>.jsonl`, eventi `token_count` con `rate_limits.primary` (5 ore) e
//! `secondary` (7 giorni), `used_percent` e `resets_at` in secondi).

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::EventoTurno;

fn casa() -> Option<PathBuf> {
    if let Some(h) = std::env::var_os("CODEX_HOME").filter(|h| !h.is_empty()) {
        return Some(PathBuf::from(h));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex"))
}

/// Il file della sessione: si cerca nelle cartelle dei giorni (le più recenti per prime).
fn file_della_sessione(sessioni: &Path, thread_id: &str) -> Option<PathBuf> {
    let fine = format!("{thread_id}.jsonl");
    let figli = |p: &Path| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(p)
            .map(|r| r.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default();
        v.sort();
        v.reverse();
        v
    };
    for anno in figli(sessioni).into_iter().take(2) {
        for mese in figli(&anno).into_iter().take(2) {
            for giorno in figli(&mese).into_iter().take(3) {
                if let Some(f) = figli(&giorno).into_iter().find(|f| {
                    f.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.ends_with(&fine))
                }) {
                    return Some(f);
                }
            }
        }
    }
    None
}

/// Le quote dall'ultimo `rate_limits` di un testo di sessione.
pub fn quote_da(testo: &str) -> Vec<EventoTurno> {
    let Some(limiti) = testo.lines().rev().find_map(|l| {
        if !l.contains("\"rate_limits\"") {
            return None;
        }
        let v: Value = serde_json::from_str(l).ok()?;
        let r = v["payload"]["rate_limits"].clone();
        r.is_object().then_some(r)
    }) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (chiave, nome) in [("primary", "five_hour"), ("secondary", "seven_day")] {
        let f = &limiti[chiave];
        let Some(p) = f["used_percent"].as_f64() else {
            continue;
        };
        let nome = match f["window_minutes"].as_i64() {
            Some(300) => "five_hour",
            Some(10080) => "seven_day",
            _ => nome,
        };
        out.push(EventoTurno::Quota {
            finestra: nome.to_string(),
            utilizzo: (p / 100.0).clamp(0.0, 1.0),
            azzera_ms: f["resets_at"].as_i64().map(|s| s.saturating_mul(1000)),
        });
    }
    out
}

/// Le quote della sessione `thread_id` (vuoto se il file non c'è).
pub fn quote(thread_id: &str) -> Vec<EventoTurno> {
    let Some(c) = casa() else {
        return Vec::new();
    };
    let Some(f) = file_della_sessione(&c.join("sessions"), thread_id) else {
        return Vec::new();
    };
    std::fs::read_to_string(f)
        .map(|t| quote_da(&t))
        .unwrap_or_default()
}

#[cfg(test)]
mod prove {
    use super::*;

    #[test]
    fn quote_dal_file_della_sessione() {
        // Righe come quelle di codex 0.157.1 (valori del 2026-10-25).
        let t = concat!(
            r#"{"type":"event_msg","payload":{"type":"token_count","rate_limits":{"limit_id":"codex","primary":{"used_percent":10.0,"window_minutes":300,"resets_at":1},"secondary":{"used_percent":1.0,"window_minutes":10080,"resets_at":2}}}}"#,
            "\n",
            r#"{"type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":42.5,"window_minutes":300,"resets_at":1792922045},"secondary":{"used_percent":3.0,"window_minutes":10080,"resets_at":1793428833}}}}"#,
            "\n",
            r#"{"type":"response_item","payload":{}}"#,
        );
        let q = quote_da(t);
        assert_eq!(
            q,
            vec![
                EventoTurno::Quota {
                    finestra: "five_hour".into(),
                    utilizzo: 0.425,
                    azzera_ms: Some(1_792_922_045_000)
                },
                EventoTurno::Quota {
                    finestra: "seven_day".into(),
                    utilizzo: 0.03,
                    azzera_ms: Some(1_793_428_833_000)
                },
            ]
        );
        assert!(quote_da("{}").is_empty());
    }
}
