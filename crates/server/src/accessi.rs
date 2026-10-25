//! «Entra» ed «Esci» dei client ufficiali (Fase 2.I). Sul server il comando di accesso parte qui
//! (`xbot_provider::accesso`); su un computer collegato parte dal lavoratore con il frame `accedi`
//! e torna con i frame `accesso`. Lo stato di ogni accesso (provider, macchina) sta in memoria e
//! si legge con `GET /provider/{id}/accesso`. L'account dei client del server si rilegge
//! all'avvio e ogni 5 minuti.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::process::ChildStdin;
use tokio::sync::oneshot;
use xbot_core::{Codice, Errore, Risposta, nuovo_id, ora_ms};
use xbot_provider::{Provider, accesso};

use crate::stato::Stato;

/// Quanto aspetta il processo di accesso il ritorno dal browser.
pub const SCADENZA: Duration = Duration::from_secs(300);
/// Quanto si aspetta l'url stampato dal client (o la prima risposta del Mac).
const ATTESA_URL: Duration = Duration::from_secs(25);

type Chiave = (String, String);

#[derive(Default)]
struct Mappe {
    /// (provider, macchina) → `{stato, url, codice_utente, chiede_codice, account, aggiornato_ms}`.
    stati: HashMap<Chiave, Value>,
    /// Lo stdin del comando di accesso sul server (per il codice di Claude Code).
    stdin: HashMap<Chiave, ChildStdin>,
    /// Le richieste al Mac che aspettano la prima risposta.
    attese: HashMap<String, oneshot::Sender<Value>>,
}

#[derive(Default)]
pub struct Accessi(Mutex<Mappe>);

impl Accessi {
    /// Smette di aspettare la risposta `richiesta_id` (tempo scaduto, frame non partito).
    pub(crate) fn dimentica(&self, richiesta_id: &str) {
        self.mappe().attese.remove(richiesta_id);
    }

    fn mappe(&self) -> MutexGuard<'_, Mappe> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn scrivi(&self, k: &Chiave, v: Value) {
        let mut v = v;
        v["aggiornato_ms"] = json!(ora_ms());
        self.mappe().stati.insert(k.clone(), v);
    }

    /// Lo stato dell'ultimo accesso; senza, `{stato: null}`.
    pub fn stato(&self, provider_id: &str, macchina_id: &str) -> Value {
        let k = (provider_id.to_string(), macchina_id.to_string());
        self.mappe()
            .stati
            .get(&k)
            .cloned()
            .unwrap_or_else(|| json!({ "stato": null }))
    }

    /// Un frame `accesso` dal lavoratore di `macchina_id`.
    pub fn dal_lavoratore(&self, macchina_id: &str, f: &Value) {
        // --- 2.K --- l'esito di `prova_collegamento` (frame `esito_prova`) va solo a chi aspetta.
        if f.get("strumenti").is_some() {
            let attesa = f["richiesta_id"]
                .as_str()
                .and_then(|r| self.mappe().attese.remove(r));
            if let Some(tx) = attesa {
                let _ = tx.send(f.clone());
            }
            return;
        }
        let provider = f["provider_id"].as_str().unwrap_or_default().to_string();
        let k = (provider, macchina_id.to_string());
        let mut v = json!({
            "stato": f["stato"], "url": f["url"], "codice_utente": f["codice_utente"],
            "chiede_codice": f["chiede_codice"].as_bool().unwrap_or(false),
            "account": f["account"], "messaggio": f["messaggio"], "comando": f["comando"],
        });
        v["aggiornato_ms"] = json!(ora_ms());
        let attesa = f["richiesta_id"]
            .as_str()
            .and_then(|r| self.mappe().attese.remove(r));
        self.mappe().stati.insert(k, v.clone());
        if let Some(tx) = attesa {
            let _ = tx.send(v);
        }
    }
}

fn senza_comando() -> Errore {
    Errore::non_valida("questo client non ha un comando di accesso: entra dal Terminale")
}

/// `POST /provider/{id}/accedi`: `{modo: 'link', url, scade_s, codice_utente?, chiede_codice}`
/// oppure `{modo: 'terminale', comando}`.
pub async fn accedi(s: &Stato, provider_id: &str, macchina_id: &str) -> Risposta<Value> {
    if macchina_id == s.servizi.macchina.id {
        return accedi_qui(s, provider_id).await;
    }
    let (rid, rx) = attendi(s);
    let frame = json!({ "tipo": "accedi", "richiesta_id": rid, "provider_id": provider_id });
    if !s.motore.macchine().manda(macchina_id, &frame) {
        s.accessi.mappe().attese.remove(&rid);
        return Err(Errore::nuovo(
            Codice::MacchinaNonConnessa,
            "il computer non è collegato: accendilo e riprova",
        ));
    }
    let v = risposta(s, &rid, rx).await?;
    match v["stato"].as_str() {
        Some("in_attesa") if v["url"].is_string() => Ok(json!({
            "modo": "link", "url": v["url"], "scade_s": SCADENZA.as_secs(),
            "codice_utente": v["codice_utente"], "chiede_codice": v["chiede_codice"],
        })),
        Some("terminale") => Ok(json!({ "modo": "terminale", "comando": v["comando"] })),
        _ => Err(Errore::interno(
            v["messaggio"]
                .as_str()
                .unwrap_or("l'accesso sul computer non è partito")
                .to_string(),
        )),
    }
}

/// Chi aspetta una risposta del lavoratore (`accedi`, `esci`, `prova_collegamento`).
pub(crate) fn attendi(s: &Stato) -> (String, oneshot::Receiver<Value>) {
    let rid = nuovo_id();
    let (tx, rx) = oneshot::channel();
    s.accessi.mappe().attese.insert(rid.clone(), tx);
    (rid, rx)
}

async fn risposta(s: &Stato, rid: &str, rx: oneshot::Receiver<Value>) -> Risposta<Value> {
    match tokio::time::timeout(ATTESA_URL, rx).await {
        Ok(Ok(v)) => Ok(v),
        _ => {
            s.accessi.mappe().attese.remove(rid);
            Err(Errore::interno("il computer non ha risposto in 25 secondi"))
        }
    }
}

fn provider(s: &Stato, id: &str) -> Risposta<Arc<dyn Provider>> {
    s.registro.trova(id).ok_or_else(|| {
        Errore::nuovo(
            Codice::ProviderNonTrovato,
            format!("{id} non gira su questo server"),
        )
    })
}

async fn accedi_qui(s: &Stato, provider_id: &str) -> Risposta<Value> {
    let p = provider(s, provider_id)?;
    let cmd = p.comando_accesso().ok_or_else(senza_comando)?;
    let k = (provider_id.to_string(), s.servizi.macchina.id.clone());
    let mut avviato = match accesso::avvia(&cmd, ATTESA_URL).await {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!(provider = %provider_id, errore = %e, "accesso senza url: comando per il Terminale");
            return Ok(json!({ "modo": "terminale", "comando": cmd.testo() }));
        }
    };
    let invito = avviato.invito.clone();
    if cmd.chiede_codice
        && let Some(i) = avviato.prendi_stdin()
    {
        s.accessi.mappe().stdin.insert(k.clone(), i);
    }
    s.accessi.scrivi(
        &k,
        json!({ "stato": "in_attesa", "url": invito.url, "codice_utente": invito.codice_utente,
                "chiede_codice": cmd.chiede_codice, "account": null }),
    );
    tracing::info!(provider = %provider_id, "accesso del client avviato sul server");
    let s2 = s.clone();
    tokio::spawn(async move {
        let esito = avviato.attendi(SCADENZA).await;
        s2.accessi.mappe().stdin.remove(&k);
        let account = leggi_account(&s2, &p).await;
        let finale = if esito == "riuscito" && account.is_null() {
            "fallito"
        } else {
            esito
        };
        s2.accessi
            .scrivi(&k, json!({ "stato": finale, "account": account }));
        tracing::info!(provider = %k.0, esito = finale, "accesso del client finito");
    });
    Ok(json!({
        "modo": "link", "url": invito.url, "scade_s": SCADENZA.as_secs(),
        "codice_utente": invito.codice_utente, "chiede_codice": cmd.chiede_codice,
    }))
}

/// `POST /provider/{id}/accesso {macchina_id, codice}`: il codice mostrato dal browser.
pub async fn codice(s: &Stato, provider_id: &str, macchina_id: &str, codice: &str) -> Risposta<()> {
    if codice.trim().is_empty() || codice.len() > 2000 {
        return Err(Errore::non_valida("codice vuoto o troppo lungo"));
    }
    let k = (provider_id.to_string(), macchina_id.to_string());
    if macchina_id != s.servizi.macchina.id {
        let f = json!({ "tipo": "codice_accesso", "provider_id": provider_id, "codice": codice });
        return if s.motore.macchine().manda(macchina_id, &f) {
            Ok(())
        } else {
            Err(Errore::nuovo(
                Codice::MacchinaNonConnessa,
                "il computer non è collegato",
            ))
        };
    }
    let preso = s.accessi.mappe().stdin.remove(&k);
    let mut stdin =
        preso.ok_or_else(|| Errore::non_valida("nessun accesso che aspetta un codice"))?;
    let r = accesso::scrivi_codice(&mut stdin, codice).await;
    s.accessi.mappe().stdin.insert(k, stdin);
    r
}

/// `POST /provider/{id}/esci`: il client esce; risposta con l'account tolto.
pub async fn esci(s: &Stato, provider_id: &str, macchina_id: &str) -> Risposta<Value> {
    let k = (provider_id.to_string(), macchina_id.to_string());
    if macchina_id != s.servizi.macchina.id {
        let (rid, rx) = attendi(s);
        let f = json!({ "tipo": "esci", "richiesta_id": rid, "provider_id": provider_id });
        if !s.motore.macchine().manda(macchina_id, &f) {
            s.accessi.mappe().attese.remove(&rid);
            return Err(Errore::nuovo(
                Codice::MacchinaNonConnessa,
                "il computer non è collegato",
            ));
        }
        let v = risposta(s, &rid, rx).await?;
        if v["stato"] != "uscito" {
            return Err(Errore::interno(
                v["messaggio"]
                    .as_str()
                    .unwrap_or("uscita non riuscita")
                    .to_string(),
            ));
        }
        return Ok(json!({ "stato": "uscito", "account": v["account"] }));
    }
    let p = provider(s, provider_id)?;
    let cmd = p.comando_uscita().ok_or_else(senza_comando)?;
    let argomenti: Vec<&str> = cmd.argomenti.iter().map(String::as_str).collect();
    let (ok, _) = accesso::uscita(&cmd.programma, &argomenti).await?;
    if !ok {
        return Err(Errore::interno("il client non è uscito"));
    }
    let account = leggi_account(s, &p).await;
    s.accessi
        .scrivi(&k, json!({ "stato": "uscito", "account": account }));
    tracing::info!(provider = %provider_id, "client uscito sul server");
    Ok(json!({ "stato": "uscito", "account": account }))
}

/// Rilegge l'account (e la versione) di un client del server e lo salva.
async fn leggi_account(s: &Stato, p: &Arc<dyn Provider>) -> Value {
    let account = match tokio::time::timeout(Duration::from_secs(30), p.account()).await {
        Ok(Ok(a)) => a.map_or(Value::Null, |a| json!(a)),
        Ok(Err(e)) => {
            tracing::warn!(provider = p.id(), errore = %e, "account non letto");
            Value::Null
        }
        Err(_) => Value::Null,
    };
    let versione = versione(p.programma()).await;
    let (id, a2) = (p.id().to_string(), account.clone());
    let _ =
        s.db.esegui(move |c| {
            xbot_db::provider::imposta_account_server(
                c,
                &id,
                (!a2.is_null()).then_some(&a2),
                versione.as_deref(),
            )
        })
        .await;
    account
}

async fn versione(programma: &str) -> Option<String> {
    let p = std::path::Path::new(programma);
    if !p.is_absolute() {
        return None;
    }
    let (ok, t) = accesso::uscita(p, &["--version"]).await.ok()?;
    ok.then(|| t.lines().next().unwrap_or_default().trim().to_string())
        .filter(|v| !v.is_empty())
}

/// Ogni 5 minuti (e subito) l'account dei client del server.
pub fn avvia_lettore(s: &Stato) {
    let s = s.clone();
    tokio::spawn(async move {
        let mut giro = tokio::time::interval(Duration::from_secs(300));
        loop {
            giro.tick().await;
            for id in s.registro.ids() {
                if let Some(p) = s.registro.trova(&id) {
                    leggi_account(&s, &p).await;
                }
            }
        }
    });
}
