//! Traduce le righe `stream-json` di Claude Code in `EventoTurno`. Nessun processo qui:
//! solo testo in ingresso, eventi in uscita (si prova con il campione registrato).

use std::collections::HashMap;

use serde_json::Value;

use crate::EventoTurno;

/// Un blocco `tool_use` aperto: l'input arriva a pezzi con `input_json_delta`.
#[derive(Debug, Default)]
struct BloccoStrumento {
    id: String,
    nome: String,
    input_iniziale: Value,
    parziale: String,
}

/// Lo stato della lettura di un turno.
#[derive(Debug, Default)]
pub struct Lettore {
    /// Blocchi `tool_use` aperti, per indice del blocco nel messaggio.
    aperti: HashMap<u64, BloccoStrumento>,
    /// `tool_use_id` → nome dello strumento, per gli esiti.
    nomi: HashMap<String, String>,
    /// Il testo della bolla corrente (dall'ultimo strumento), come lo tiene il traduttore.
    bolla: String,
    /// Vero se il blocco di testo corrente ha già ricevuto delta.
    delta_nel_blocco: bool,
    sessione: Option<String>,
    finito: bool,
}

fn testo(v: &Value) -> Option<&str> {
    v.as_str()
}

impl Lettore {
    pub fn nuovo() -> Self {
        Self::default()
    }

    /// Vero dopo la riga `result` (riuscita o fallita).
    pub fn finito(&self) -> bool {
        self.finito
    }

    /// L'id di sessione visto finora.
    pub fn sessione(&self) -> Option<&str> {
        self.sessione.as_deref()
    }

    /// Una riga di stdout. Le righe che non sono JSON o che non servono si ignorano.
    pub fn riga(&mut self, linea: &str) -> Vec<EventoTurno> {
        let linea = linea.trim();
        if linea.is_empty() {
            return Vec::new();
        }
        let v: Value = match serde_json::from_str(linea) {
            Ok(v) => v,
            Err(e) => {
                tracing::debug!(errore = %e, lunghezza = linea.len(), "riga non JSON ignorata");
                return Vec::new();
            }
        };
        if let Some(s) = testo(&v["session_id"]) {
            self.sessione = Some(s.to_string());
        }
        // Le righe dei sottoagenti (Task) hanno `parent_tool_use_id`: non vanno nella bolla.
        if !v["parent_tool_use_id"].is_null() {
            tracing::debug!("riga di un sottoagente ignorata");
            return Vec::new();
        }
        let tipo = testo(&v["type"]).unwrap_or_default();
        match tipo {
            "system" if testo(&v["subtype"]) == Some("init") => vec![EventoTurno::Avviato {
                sessione_esterna_id: testo(&v["session_id"]).map(str::to_string),
            }],
            "stream_event" => self.evento_flusso(&v["event"]),
            "assistant" => self.assistente(&v["message"]),
            "user" => self.utente(&v["message"]),
            "rate_limit_event" => quote(&v["rate_limit_info"]),
            "result" => self.risultato(&v),
            altro => {
                tracing::debug!(tipo = altro, sottotipo = ?v["subtype"].as_str(), "riga ignorata");
                Vec::new()
            }
        }
    }

    fn evento_flusso(&mut self, e: &Value) -> Vec<EventoTurno> {
        let indice = e["index"].as_u64().unwrap_or_default();
        match testo(&e["type"]).unwrap_or_default() {
            "content_block_start" => {
                let b = &e["content_block"];
                match testo(&b["type"]) {
                    Some("tool_use") => {
                        self.aperti.insert(
                            indice,
                            BloccoStrumento {
                                id: testo(&b["id"]).unwrap_or_default().to_string(),
                                nome: testo(&b["name"]).unwrap_or("strumento").to_string(),
                                input_iniziale: b["input"].clone(),
                                parziale: String::new(),
                            },
                        );
                    }
                    Some("text") => self.delta_nel_blocco = false,
                    _ => {}
                }
                Vec::new()
            }
            "content_block_delta" => {
                let d = &e["delta"];
                match testo(&d["type"]).unwrap_or_default() {
                    "text_delta" => {
                        let t = testo(&d["text"]).unwrap_or_default().to_string();
                        self.delta_nel_blocco = true;
                        self.bolla.push_str(&t);
                        vec![EventoTurno::TestoDelta(t)]
                    }
                    "thinking_delta" => {
                        let t = testo(&d["thinking"]).unwrap_or_default().to_string();
                        vec![EventoTurno::Ragionamento(t)]
                    }
                    "input_json_delta" => {
                        if let Some(b) = self.aperti.get_mut(&indice) {
                            b.parziale
                                .push_str(testo(&d["partial_json"]).unwrap_or_default());
                        }
                        Vec::new()
                    }
                    _ => Vec::new(),
                }
            }
            "content_block_stop" => match self.aperti.remove(&indice) {
                Some(b) => vec![self.chiudi_strumento(b)],
                None => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    fn chiudi_strumento(&mut self, b: BloccoStrumento) -> EventoTurno {
        let input_json = if b.parziale.trim().is_empty() {
            match b.input_iniziale {
                Value::Null => Value::Object(serde_json::Map::new()),
                v => v,
            }
        } else {
            serde_json::from_str(&b.parziale).unwrap_or(Value::String(b.parziale))
        };
        self.nomi.insert(b.id, b.nome.clone());
        // Dopo uno strumento il testo va in una bolla nuova (come nel traduttore del motore).
        self.bolla.clear();
        EventoTurno::ChiamataStrumento {
            nome: b.nome,
            input_json,
        }
    }

    /// Il messaggio intero dell'assistente: il testo della bolla, così come deve risultare.
    /// Con i delta già arrivati è lo stesso testo; senza delta (blocco intero) lo aggiunge.
    fn assistente(&mut self, m: &Value) -> Vec<EventoTurno> {
        let Some(blocchi) = m["content"].as_array() else {
            return Vec::new();
        };
        let mut visto = false;
        for b in blocchi {
            if testo(&b["type"]) == Some("text") {
                visto = true;
                if !self.delta_nel_blocco {
                    self.bolla.push_str(testo(&b["text"]).unwrap_or_default());
                }
            }
        }
        if visto && !self.bolla.is_empty() {
            vec![EventoTurno::TestoCompleto(self.bolla.clone())]
        } else {
            Vec::new()
        }
    }

    fn utente(&mut self, m: &Value) -> Vec<EventoTurno> {
        let Some(blocchi) = m["content"].as_array() else {
            return Vec::new();
        };
        blocchi
            .iter()
            .filter(|b| testo(&b["type"]) == Some("tool_result"))
            .map(|b| {
                let id = testo(&b["tool_use_id"]).unwrap_or_default();
                let nome = self
                    .nomi
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| "strumento".into());
                EventoTurno::EsitoStrumento {
                    nome,
                    esito_json: b["content"].clone(),
                    ok: !b["is_error"].as_bool().unwrap_or(false),
                }
            })
            .collect()
    }

    fn risultato(&mut self, v: &Value) -> Vec<EventoTurno> {
        self.finito = true;
        let sottotipo = testo(&v["subtype"]).unwrap_or("sconosciuto");
        if v["is_error"].as_bool().unwrap_or(false) || sottotipo != "success" {
            let testo_errore = match &v["result"] {
                Value::String(s) if !s.is_empty() => s.clone(),
                _ => match &v["errors"] {
                    Value::Array(a) if !a.is_empty() => a
                        .iter()
                        .map(|e| e.as_str().map_or_else(|| e.to_string(), str::to_string))
                        .collect::<Vec<_>>()
                        .join("; "),
                    _ => format!("Claude Code ha chiuso con {sottotipo}"),
                },
            };
            let codice = if sottotipo == "success" {
                "errore"
            } else {
                sottotipo
            };
            return vec![EventoTurno::Fallito {
                codice: codice.to_string(),
                testo: testo_errore,
            }];
        }
        let u = &v["usage"];
        let n = |k: &str| u[k].as_i64();
        let cache_lettura = n("cache_read_input_tokens");
        let ingresso = [
            n("input_tokens"),
            cache_lettura,
            n("cache_creation_input_tokens"),
        ];
        let token_ingresso = ingresso
            .iter()
            .any(Option::is_some)
            .then(|| ingresso.iter().flatten().sum());
        vec![EventoTurno::Finito {
            token_ingresso,
            token_uscita: n("output_tokens"),
            token_cache: cache_lettura,
            costo_usd: v["total_cost_usd"].as_f64(),
            sessione_esterna_id: testo(&v["session_id"])
                .map(str::to_string)
                .or_else(|| self.sessione.clone()),
        }]
    }
}

/// `rate_limit_info.unifiedWindows`: una `Quota` per finestra, prima `five_hour` poi `seven_day`.
fn quote(info: &Value) -> Vec<EventoTurno> {
    let finestre = &info["unifiedWindows"];
    let mut v = Vec::new();
    for nome in ["five_hour", "seven_day"] {
        let f = &finestre[nome];
        let Some(utilizzo) = f["utilization"].as_f64() else {
            continue;
        };
        v.push(EventoTurno::Quota {
            finestra: nome.to_string(),
            utilizzo,
            azzera_ms: f["resetsAt"].as_i64().map(|s| s.saturating_mul(1000)),
        });
    }
    if v.is_empty() {
        tracing::debug!("rate_limit_event senza unifiedWindows");
    }
    v
}

#[cfg(test)]
mod test {
    use super::*;

    const CAMPIONE: &str = include_str!("../../prove/claude-stream-esempio.jsonl");

    fn tutto(righe: &str) -> (Vec<EventoTurno>, Lettore) {
        let mut l = Lettore::nuovo();
        let v = righe.lines().flat_map(|r| l.riga(r)).collect();
        (v, l)
    }

    #[test]
    fn campione_registrato() {
        let (v, l) = tutto(CAMPIONE);
        let sid = "fd012275-2c09-4b69-8765-c8e509475aba";
        assert_eq!(
            v,
            vec![
                EventoTurno::Avviato {
                    sessione_esterna_id: Some(sid.into())
                },
                EventoTurno::TestoDelta("ciao".into()),
                EventoTurno::TestoCompleto("ciao".into()),
                EventoTurno::Quota {
                    finestra: "five_hour".into(),
                    utilizzo: 0.12,
                    azzera_ms: Some(1_792_857_600_000)
                },
                EventoTurno::Quota {
                    finestra: "seven_day".into(),
                    utilizzo: 0.24,
                    azzera_ms: Some(1_793_124_000_000)
                },
                EventoTurno::Finito {
                    token_ingresso: Some(2 + 23_919 + 28_018),
                    token_uscita: Some(5),
                    token_cache: Some(23_919),
                    costo_usd: Some(0.005_845_49),
                    sessione_esterna_id: Some(sid.into()),
                },
            ]
        );
        assert!(l.finito());
        assert_eq!(l.sessione(), Some(sid));
    }

    #[test]
    fn strumento_a_pezzi_ed_esito() {
        let righe = r#"
{"type":"stream_event","event":{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"Bash","input":{}}},"parent_tool_use_id":null}
{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"command\":"}},"parent_tool_use_id":null}
{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"ls\"}"}},"parent_tool_use_id":null}
{"type":"stream_event","event":{"type":"content_block_stop","index":1},"parent_tool_use_id":null}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"a\nb","is_error":false}]},"parent_tool_use_id":null}
{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"x"}},"parent_tool_use_id":"toolu_9"}
non json
{"type":"qualcosa_di_nuovo"}
"#;
        let (v, _) = tutto(righe);
        assert_eq!(
            v,
            vec![
                EventoTurno::ChiamataStrumento {
                    nome: "Bash".into(),
                    input_json: serde_json::json!({ "command": "ls" })
                },
                EventoTurno::EsitoStrumento {
                    nome: "Bash".into(),
                    esito_json: Value::String("a\nb".into()),
                    ok: true
                },
            ]
        );
    }

    #[test]
    fn risultato_con_errore() {
        let (v, l) = tutto(
            r#"{"type":"result","subtype":"error_max_turns","is_error":true,"session_id":"s"}"#,
        );
        assert!(l.finito());
        assert!(matches!(
            &v[0],
            EventoTurno::Fallito { codice, .. } if codice == "error_max_turns"
        ));
        let (v, _) = tutto(
            r#"{"type":"result","subtype":"success","is_error":true,"result":"API Error: 529"}"#,
        );
        assert_eq!(
            v[0],
            EventoTurno::Fallito {
                codice: "errore".into(),
                testo: "API Error: 529".into()
            }
        );
    }

    #[test]
    fn testo_intero_senza_delta() {
        let righe = r#"
{"type":"assistant","message":{"content":[{"type":"text","text":"tutto "},{"type":"text","text":"insieme"}]},"parent_tool_use_id":null}
"#;
        let (v, _) = tutto(righe);
        assert_eq!(v, vec![EventoTurno::TestoCompleto("tutto insieme".into())]);
    }
}
