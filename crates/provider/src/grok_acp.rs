//! Grok in modo agente (Fase 2.G): `grok agent [--always-approve] -m <modello> stdio`, protocollo
//! ACP (JSON-RPC 2.0, una riga per messaggio). Un processo per sessione, tenuto caldo da `caldi`.
//! Formato visto con grok 1.0.70 il 2026-10-25 (campione `prove/grok-acp-esempio.jsonl`):
//! `initialize` → `session/new {cwd, mcpServers, _meta.rules}` (l'id lo sceglie grok) oppure
//! `session/load {sessionId, cwd, mcpServers}` (prima della risposta ripete la storia come
//! `session/update`, che si ignora) → `session/set_config_option {configId: reasoning_effort}`.
//! Ogni turno è `session/prompt`: arrivano `session/update` (`agent_message_chunk`,
//! `agent_thought_chunk`, `tool_call`, `tool_call_update`) e le richieste
//! `session/request_permission` (opzioni `allow-once`, `always-allow`, `reject-once`,
//! `reject-always`); la risposta al prompt porta `stopReason` e `_meta.usage` del turno
//! (`costUsdTicks`: 10^10 per dollaro, come `total_cost_usd_ticks` di headless). Dopo un rifiuto
//! lo `stopReason` è `cancelled`: è un rifiuto, non un'interruzione. Le notifiche `_x.ai/mcp*`
//! dicono quando i collegamenti sono pronti (`pronti`); le richieste sconosciute: errore -32601.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{mpsc, oneshot};

use crate::esterni::{Permesso, intero};
use crate::{EventoTurno, Permessi, caldi};

/// Dollari per tick nei costi di grok.
const TICK_PER_DOLLARO: f64 = 1e10;

/// La traduzione degli aggiornamenti di un turno (senza processo: si prova col campione).
#[derive(Debug, Default)]
pub struct Traduttore {
    nomi: HashMap<String, (String, Value)>,
    /// Vero dopo un permesso negato: il `cancelled` che segue non è un'interruzione.
    pub negato: bool,
}

/// Il nome alla Claude Code di uno strumento ACP (`variant` di `rawInput`, altrimenti dal `kind`).
pub fn nome_strumento(t: &Value) -> String {
    if let Some(v) = t["rawInput"]["variant"].as_str() {
        return v.to_string();
    }
    let kind = t["kind"]
        .as_str()
        .or(t["_meta"]["x.ai/tool"]["kind"].as_str());
    match kind {
        Some("execute") => "Bash".into(),
        Some("edit" | "delete" | "move") => "Edit".into(),
        Some("read") => "Read".into(),
        Some("fetch") => "WebFetch".into(),
        _ => t["title"].as_str().unwrap_or("strumento").to_string(),
    }
}

/// L'input di uno strumento, senza la `variant` aggiunta da grok.
pub fn input_strumento(t: &Value) -> Value {
    let mut i = t["rawInput"].clone();
    if let Some(o) = i.as_object_mut() {
        o.remove("variant");
        o.remove("is_background");
    }
    i
}

impl Traduttore {
    /// Un `session/update` del turno.
    pub fn aggiornamento(&mut self, u: &Value) -> Vec<EventoTurno> {
        let testo = |u: &Value| {
            u["content"]["text"]
                .as_str()
                .filter(|t| !t.is_empty())
                .map(str::to_string)
        };
        match u["sessionUpdate"].as_str().unwrap_or_default() {
            "agent_message_chunk" => testo(u).map(EventoTurno::TestoDelta).into_iter().collect(),
            "agent_thought_chunk" => testo(u)
                .map(EventoTurno::Ragionamento)
                .into_iter()
                .collect(),
            "tool_call" => {
                let id = u["toolCallId"].as_str().unwrap_or_default().to_string();
                let nome = u["title"]
                    .as_str()
                    .filter(|t| !t.is_empty())
                    .unwrap_or("strumento")
                    .to_string();
                let input = input_strumento(u);
                self.nomi.insert(id, (nome.clone(), input.clone()));
                let mut out = vec![EventoTurno::ChiamataStrumento {
                    nome,
                    input_json: input,
                }];
                out.extend(self.chiusura(u));
                out
            }
            "tool_call_update" => self.chiusura(u),
            _ => Vec::new(),
        }
    }

    fn chiusura(&mut self, u: &Value) -> Vec<EventoTurno> {
        let stato = u["status"].as_str().unwrap_or_default();
        if stato != "completed" && stato != "failed" {
            return Vec::new();
        }
        let id = u["toolCallId"].as_str().unwrap_or_default();
        let (nome, _) = self
            .nomi
            .remove(id)
            .unwrap_or_else(|| ("strumento".to_string(), Value::Null));
        let esito = match &u["rawOutput"] {
            Value::Object(o) if o.contains_key("output_for_prompt") => {
                o["output_for_prompt"].clone()
            }
            Value::Null => u["content"].clone(),
            altro => altro.clone(),
        };
        vec![EventoTurno::EsitoStrumento {
            nome,
            esito_json: esito,
            ok: stato == "completed",
        }]
    }

    /// La risposta a `session/prompt`.
    pub fn fine(&mut self, risultato: &Value, sessione: &str) -> Vec<EventoTurno> {
        let mut out = Vec::new();
        for (_, (nome, _)) in self.nomi.drain() {
            out.push(EventoTurno::EsitoStrumento {
                nome,
                esito_json: Value::Null,
                ok: false,
            });
        }
        match risultato["stopReason"].as_str().unwrap_or("end_turn") {
            "cancelled" if !self.negato => {
                out.push(EventoTurno::Fallito {
                    codice: "interrotto".into(),
                    testo: "Grok ha interrotto il turno".into(),
                });
                return out;
            }
            "end_turn" | "cancelled" => {}
            "max_tokens" => out.push(EventoTurno::Avviso(
                "Grok si è fermato al limite di lunghezza della risposta".into(),
            )),
            "max_turn_requests" => out.push(EventoTurno::Avviso(
                "Grok si è fermato al numero massimo di giri".into(),
            )),
            "refusal" => out.push(EventoTurno::Avviso("Grok ha rifiutato la richiesta".into())),
            altro => out.push(EventoTurno::Avviso(format!(
                "Grok ha chiuso il turno con «{altro}»"
            ))),
        }
        let u = &risultato["_meta"]["usage"];
        let ingresso = intero(&u["inputTokens"]);
        let cache = intero(&u["cachedReadTokens"]);
        out.push(EventoTurno::Finito {
            // Come in headless: i token d'ingresso senza quelli letti dalla cache.
            token_ingresso: ingresso.map(|i| i - cache.unwrap_or(0)),
            token_uscita: intero(&u["outputTokens"]),
            token_cache: cache,
            costo_usd: u["costUsdTicks"].as_f64().map(|t| t / TICK_PER_DOLLARO),
            sessione_esterna_id: Some(sessione.to_string()),
        });
        out
    }
}

/// Come aprire la sessione ACP la prima volta.
#[derive(Debug, Clone)]
pub struct Apertura {
    pub cartella: PathBuf,
    /// `sessione_esterna_id` da riprendere con `session/load`.
    pub riprendi: Option<String>,
    pub regole: Option<String>,
    pub livello: Option<String>,
}

/// Il messaggio di un turno.
pub struct Messaggio {
    pub testo: String,
    pub permesso: Permesso,
    pub chiedi: Option<Permessi>,
}

/// Il processo `grok agent stdio` di una sessione.
pub struct Processo {
    figlio: Child,
    stdin: Option<ChildStdin>,
    righe: tokio::io::Lines<BufReader<ChildStdout>>,
    prossimo: u64,
    apertura: Option<Apertura>,
    sessione: String,
    silenzio: Duration,
    attesa_kill: Duration,
    pronti: pronti::Pronti,
}

/// Perché la lettura si è fermata.
enum Stop {
    Interrotto,
    Silenzio,
    Uscito(String),
}

/// Lo stato di un turno durante la lettura.
struct Turno<'a> {
    eventi: &'a mpsc::Sender<EventoTurno>,
    stop: &'a mut oneshot::Receiver<()>,
    ascolta_stop: bool,
    traduttore: Traduttore,
    permesso: Permesso,
    chiedi: Option<Permessi>,
}

impl Processo {
    pub fn nuovo(
        mut figlio: Child,
        apertura: Apertura,
        silenzio: Duration,
        attesa_kill: Duration,
    ) -> Option<Self> {
        let stdout = figlio.stdout.take()?;
        Some(Self {
            stdin: figlio.stdin.take(),
            figlio,
            righe: BufReader::new(stdout).lines(),
            prossimo: 1,
            apertura: Some(apertura),
            sessione: String::new(),
            silenzio,
            attesa_kill,
            pronti: pronti::Pronti::default(),
        })
    }

    async fn scrivi(&mut self, v: &Value) -> Result<(), String> {
        let s = self.stdin.as_mut().ok_or("stdin di grok chiuso")?;
        let riga = format!("{v}\n");
        s.write_all(riga.as_bytes())
            .await
            .map_err(|e| format!("stdin di grok: {e}"))?;
        s.flush().await.map_err(|e| format!("stdin di grok: {e}"))
    }

    /// Manda una richiesta e legge fino alla sua risposta. Durante un turno (`t`) traduce gli
    /// aggiornamenti e decide i permessi.
    async fn chiama(
        &mut self,
        metodo: &str,
        parametri: Value,
        mut t: Option<&mut Turno<'_>>,
    ) -> Result<Result<Value, Value>, Stop> {
        let id = self.prossimo;
        self.prossimo += 1;
        let r = json!({ "jsonrpc": "2.0", "id": id, "method": metodo, "params": parametri });
        if let Err(e) = self.scrivi(&r).await {
            return Err(Stop::Uscito(e));
        }
        loop {
            let linea = {
                let prossima = match t.as_deref_mut() {
                    Some(tt) => {
                        let ascolta = tt.ascolta_stop;
                        tokio::select! {
                            r = &mut *tt.stop, if ascolta => {
                                if r.is_ok() {
                                    return Err(Stop::Interrotto);
                                }
                                tt.ascolta_stop = false;
                                continue;
                            }
                            r = tokio::time::timeout(self.silenzio, self.righe.next_line()) => r,
                        }
                    }
                    None => tokio::time::timeout(self.silenzio, self.righe.next_line()).await,
                };
                match prossima {
                    Err(_) => return Err(Stop::Silenzio),
                    Ok(Err(e)) => return Err(Stop::Uscito(format!("lettura di stdout: {e}"))),
                    Ok(Ok(None)) => return Err(Stop::Uscito(String::new())),
                    Ok(Ok(Some(l))) => l,
                }
            };
            let Ok(v) = serde_json::from_str::<Value>(linea.trim()) else {
                continue;
            };
            let metodo_in = v["method"].as_str().unwrap_or_default().to_string();
            if metodo_in.is_empty() {
                if v["id"].as_u64() == Some(id) {
                    return Ok(match v.get("error") {
                        Some(e) => Err(e.clone()),
                        None => Ok(v["result"].clone()),
                    });
                }
                continue;
            }
            let richiesta = v.get("id").cloned().filter(|i| !i.is_null());
            match (metodo_in.as_str(), richiesta) {
                ("session/update", None) => {
                    if let Some(tt) = t.as_deref_mut()
                        && v["params"]["sessionId"].as_str() == Some(self.sessione.as_str())
                    {
                        for ev in tt.traduttore.aggiornamento(&v["params"]["update"]) {
                            if tt.eventi.send(ev).await.is_err() {
                                return Err(Stop::Interrotto);
                            }
                        }
                    }
                }
                ("session/request_permission", Some(rid)) => {
                    let scelta = match t.as_deref_mut() {
                        Some(tt) => decidi(tt, &v["params"]).await?,
                        None => "reject-once",
                    };
                    let risposta = json!({ "jsonrpc": "2.0", "id": rid, "result": {
                        "outcome": { "outcome": "selected", "optionId": scelta } } });
                    if let Err(e) = self.scrivi(&risposta).await {
                        return Err(Stop::Uscito(e));
                    }
                }
                (m, Some(rid)) => {
                    tracing::debug!(metodo = m, "richiesta ACP non gestita: errore -32601");
                    let risposta = json!({ "jsonrpc": "2.0", "id": rid, "error": {
                        "code": -32601, "message": "metodo non gestito dal client XBot" } });
                    if let Err(e) = self.scrivi(&risposta).await {
                        return Err(Stop::Uscito(e));
                    }
                }
                (m, None) => self.pronti.vedi(m, &v["params"]),
            }
        }
    }

    /// `initialize` e la sessione (nuova o ripresa), la prima volta.
    async fn apri(
        &mut self,
        eventi: &mpsc::Sender<EventoTurno>,
    ) -> Result<Result<(), String>, Stop> {
        let Some(a) = self.apertura.take() else {
            return Ok(Ok(()));
        };
        let init = json!({ "protocolVersion": 1, "clientCapabilities": {
            "fs": { "readTextFile": false, "writeTextFile": false }, "terminal": false } });
        if let Err(e) = self.chiama("initialize", init, None).await? {
            return Ok(Err(errore_testo(&e)));
        }
        let cwd = a.cartella.to_string_lossy().into_owned();
        let mut aperta = false;
        if let Some(id) = &a.riprendi {
            let p = json!({ "sessionId": id, "cwd": cwd, "mcpServers": [] });
            match self.chiama("session/load", p, None).await? {
                Ok(_) => {
                    self.sessione = id.clone();
                    aperta = true;
                }
                Err(e) => {
                    tracing::warn!(errore = %errore_testo(&e), "sessione di grok non ripresa");
                    let _ = eventi
                        .send(EventoTurno::Avviso(
                            "Grok non ha ritrovato la conversazione di prima: si riparte da una nuova"
                                .into(),
                        ))
                        .await;
                }
            }
        }
        if !aperta {
            let mut p = json!({ "cwd": cwd, "mcpServers": [] });
            if let Some(r) = a.regole.as_deref().filter(|r| !r.trim().is_empty()) {
                p["_meta"] = json!({ "rules": r.trim().replace('\0', "") });
            }
            match self.chiama("session/new", p, None).await? {
                Ok(r) => match r["sessionId"].as_str() {
                    Some(s) => self.sessione = s.to_string(),
                    None => return Ok(Err("grok non ha dato l'id della sessione".into())),
                },
                Err(e) => return Ok(Err(errore_testo(&e))),
            }
        }
        self.aspetta_mcp(eventi).await?;
        if let Some(l) = a.livello {
            let p =
                json!({ "sessionId": self.sessione, "configId": "reasoning_effort", "value": l });
            if let Err(e) = self.chiama("session/set_config_option", p, None).await? {
                let _ = eventi
                    .send(EventoTurno::Avviso(format!(
                        "Grok non accetta il livello «{l}»: {}",
                        errore_testo(&e)
                    )))
                    .await;
            }
        }
        Ok(Ok(()))
    }

    async fn fallito(&mut self, eventi: &mpsc::Sender<EventoTurno>, s: Stop) {
        match s {
            Stop::Interrotto => {
                caldi::ferma(&mut self.figlio, self.attesa_kill).await;
            }
            Stop::Silenzio => {
                caldi::ferma(&mut self.figlio, self.attesa_kill).await;
                let minuti = self.silenzio.as_secs_f64() / 60.0;
                let _ = eventi
                    .send(EventoTurno::Fallito {
                        codice: "silenzio".into(),
                        testo: format!("grok non scrive da {minuti:.1} minuti: interrotto"),
                    })
                    .await;
            }
            Stop::Uscito(motivo) => {
                let stato = match tokio::time::timeout(self.attesa_kill, self.figlio.wait()).await {
                    Ok(Ok(s)) => s.to_string(),
                    Ok(Err(e)) => format!("attesa fallita: {e}"),
                    Err(_) => {
                        caldi::ferma(&mut self.figlio, self.attesa_kill).await;
                        "non uscito, fermato".into()
                    }
                };
                let mut testo = format!("grok è uscito senza risultato ({stato})");
                if !motivo.is_empty() {
                    testo.push_str(": ");
                    testo.push_str(&motivo);
                }
                let _ = eventi
                    .send(EventoTurno::Fallito {
                        codice: "processo_uscito".into(),
                        testo,
                    })
                    .await;
            }
        }
    }
}

fn errore_testo(e: &Value) -> String {
    let m = e["message"]
        .as_str()
        .unwrap_or("errore di grok")
        .to_string();
    let l = m.to_lowercase();
    if l.contains("auth") || l.contains("not signed in") || l.contains("login") {
        return format!(
            "grok non è autenticato su questo computer: serve `grok login` dal Terminale ({m})"
        );
    }
    m
}

/// L'opzione scelta per una richiesta di permesso.
async fn decidi(t: &mut Turno<'_>, p: &Value) -> Result<&'static str, Stop> {
    let tc = &p["toolCall"];
    let nome = nome_strumento(tc);
    let input = input_strumento(tc);
    let cosa = input["command"]
        .as_str()
        .map(|c| format!("il comando «{c}»"))
        .unwrap_or_else(|| format!("lo strumento «{nome}»"));
    let esito: Result<(), String> = match (t.permesso, &t.chiedi) {
        (Permesso::Sempre, _) => Ok(()),
        (Permesso::Mai, _) => Err(format!(
            "Grok ha negato {cosa}: il permesso della macchina è «mai»"
        )),
        (Permesso::Chiedi, None) => Err(format!(
            "Grok ha negato {cosa}: nessuno può chiedere il permesso all'utente da qui"
        )),
        (Permesso::Chiedi, Some(c)) => {
            let ascolta = t.ascolta_stop;
            tokio::select! {
                r = c.0.chiedi(&nome, &input) => r.map_err(|m| format!("Negato {cosa}: {m}")),
                r = &mut *t.stop, if ascolta => {
                    if r.is_ok() {
                        return Err(Stop::Interrotto);
                    }
                    t.ascolta_stop = false;
                    c.0.chiedi(&nome, &input).await.map_err(|m| format!("Negato {cosa}: {m}"))
                }
            }
        }
    };
    match esito {
        Ok(()) => Ok("allow-once"),
        Err(m) => {
            t.traduttore.negato = true;
            if t.eventi.send(EventoTurno::Avviso(m)).await.is_err() {
                return Err(Stop::Interrotto);
            }
            Ok("reject-once")
        }
    }
}

#[async_trait]
impl caldi::Conversazione for Processo {
    type Messaggio = Messaggio;

    async fn turno(&mut self, l: caldi::Lavoro<Messaggio>) -> caldi::DopoTurno {
        let caldi::Lavoro {
            messaggio,
            eventi,
            mut stop,
        } = l;
        match self.apri(&eventi).await {
            Ok(Ok(())) => {}
            Ok(Err(testo)) => {
                let codice = if testo.contains("grok login") {
                    "non_autenticato"
                } else {
                    "errore"
                };
                let _ = eventi
                    .send(EventoTurno::Fallito {
                        codice: codice.into(),
                        testo,
                    })
                    .await;
                return caldi::DopoTurno::Chiudi;
            }
            Err(s) => {
                self.fallito(&eventi, s).await;
                return caldi::DopoTurno::Chiudi;
            }
        }
        let _ = eventi
            .send(EventoTurno::Avviato {
                sessione_esterna_id: Some(self.sessione.clone()),
            })
            .await;
        let mut t = Turno {
            eventi: &eventi,
            stop: &mut stop,
            ascolta_stop: true,
            traduttore: Traduttore::default(),
            permesso: messaggio.permesso,
            chiedi: messaggio.chiedi,
        };
        let p = json!({ "sessionId": self.sessione,
            "prompt": [{ "type": "text", "text": messaggio.testo }] });
        let r = self.chiama("session/prompt", p, Some(&mut t)).await;
        let mut traduttore = std::mem::take(&mut t.traduttore);
        drop(t);
        match r {
            Ok(Ok(risultato)) => {
                let sessione = self.sessione.clone();
                for ev in traduttore.fine(&risultato, &sessione) {
                    let _ = eventi.send(ev).await;
                }
                if caldi::accesi() {
                    caldi::DopoTurno::Pronto
                } else {
                    caldi::DopoTurno::Chiudi
                }
            }
            Ok(Err(e)) => {
                let testo = errore_testo(&e);
                let non_autenticato = testo.contains("grok login");
                let codice = if non_autenticato {
                    "non_autenticato"
                } else {
                    "errore"
                };
                let _ = eventi
                    .send(EventoTurno::Fallito {
                        codice: codice.into(),
                        testo,
                    })
                    .await;
                if non_autenticato {
                    caldi::DopoTurno::Chiudi
                } else {
                    caldi::DopoTurno::Pronto
                }
            }
            Err(s) => {
                if matches!(s, Stop::Interrotto) {
                    drop(eventi);
                    caldi::ferma(&mut self.figlio, self.attesa_kill).await;
                } else {
                    self.fallito(&eventi, s).await;
                }
                caldi::DopoTurno::Chiudi
            }
        }
    }

    async fn uscito(&mut self) {
        let _ = self.figlio.wait().await;
    }

    async fn chiudi(&mut self) {
        self.stdin = None;
        if tokio::time::timeout(self.attesa_kill, self.figlio.wait())
            .await
            .is_err()
        {
            caldi::ferma(&mut self.figlio, self.attesa_kill).await;
        }
    }
}

#[path = "grok_acp_mcp.rs"]
mod pronti;

#[cfg(test)]
#[path = "grok_acp_prove.rs"]
mod test;
