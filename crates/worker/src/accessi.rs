//! Fase 2.I: «Entra», «Esci» e il codice di accesso dei client del Mac, chiesti dal server con i
//! frame `accedi`, `esci`, `codice_accesso`. Il comando di accesso resta vivo fino a 5 minuti;
//! l'esito torna con i frame `accesso` (mai token: solo url, codice da scrivere e account).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::process::ChildStdin;
use tokio::sync::mpsc;
use xbot_provider::{Account, Provider, accesso};

use crate::protocollo::DalLavoratore;

const ATTESA_URL: Duration = Duration::from_secs(20);
const SCADENZA: Duration = Duration::from_secs(300);

/// Un account cambiato: (provider, account).
pub type Cambio = (String, Option<Account>);

#[derive(Clone)]
pub struct Accessi {
    stdin: Arc<Mutex<HashMap<String, ChildStdin>>>,
    /// Righe da spedire così come sono (frame `accesso`).
    pub grezzi: mpsc::UnboundedSender<String>,
    pub cambi: mpsc::UnboundedSender<Cambio>,
}

#[derive(Default)]
struct Esito {
    stato: &'static str,
    url: Option<String>,
    codice_utente: Option<String>,
    chiede_codice: bool,
    account: Option<Account>,
    messaggio: Option<String>,
    comando: Option<String>,
}

impl Accessi {
    pub fn nuovo(
        grezzi: mpsc::UnboundedSender<String>,
        cambi: mpsc::UnboundedSender<Cambio>,
    ) -> Self {
        Self {
            stdin: Default::default(),
            grezzi,
            cambi,
        }
    }

    fn manda(&self, richiesta_id: Option<&str>, provider_id: &str, e: Esito) {
        let f = DalLavoratore::Accesso {
            richiesta_id: richiesta_id.map(str::to_string),
            provider_id: provider_id.to_string(),
            stato: e.stato.to_string(),
            url: e.url,
            codice_utente: e.codice_utente,
            chiede_codice: e.chiede_codice,
            account: e.account,
            messaggio: e.messaggio,
            comando: e.comando,
        };
        if let Ok(r) = serde_json::to_string(&f) {
            let _ = self.grezzi.send(r);
        }
    }

    fn errore(&self, richiesta_id: &str, provider_id: &str, messaggio: String) {
        let e = Esito {
            stato: "fallito",
            messaggio: Some(messaggio),
            ..Default::default()
        };
        self.manda(Some(richiesta_id), provider_id, e);
    }

    /// `accedi`: lancia il comando del client e risponde con l'url (poi con l'esito).
    pub fn accedi(
        &self,
        richiesta_id: String,
        provider_id: String,
        p: Result<Box<dyn Provider>, String>,
    ) {
        let me = self.clone();
        tokio::spawn(async move {
            let p = match p {
                Ok(p) => p,
                Err(e) => return me.errore(&richiesta_id, &provider_id, e),
            };
            let Some(cmd) = p.comando_accesso() else {
                return me.errore(
                    &richiesta_id,
                    &provider_id,
                    "questo client non ha un comando di accesso".into(),
                );
            };
            let mut a = match accesso::avvia(&cmd, ATTESA_URL).await {
                Ok(a) => a,
                Err(e) => {
                    tracing::warn!(provider = %provider_id, errore = %e, "accesso senza url");
                    let e = Esito {
                        stato: "terminale",
                        comando: Some(cmd.testo()),
                        ..Default::default()
                    };
                    return me.manda(Some(&richiesta_id), &provider_id, e);
                }
            };
            if cmd.chiede_codice
                && let Some(s) = a.prendi_stdin()
                && let Ok(mut m) = me.stdin.lock()
            {
                m.insert(provider_id.clone(), s);
            }
            let e = Esito {
                stato: "in_attesa",
                url: a.invito.url.clone(),
                codice_utente: a.invito.codice_utente.clone(),
                chiede_codice: cmd.chiede_codice,
                ..Default::default()
            };
            me.manda(Some(&richiesta_id), &provider_id, e);
            tracing::info!(provider = %provider_id, "accesso del client avviato");
            let esito = a.attendi(SCADENZA).await;
            if let Ok(mut m) = me.stdin.lock() {
                m.remove(&provider_id);
            }
            let account = crate::scoperta::leggi_account(p.as_ref()).await;
            let stato = if esito == "riuscito" && account.is_none() {
                "fallito"
            } else {
                esito
            };
            let _ = me.cambi.send((provider_id.clone(), account.clone()));
            let e = Esito {
                stato,
                account,
                ..Default::default()
            };
            me.manda(None, &provider_id, e);
            tracing::info!(provider = %provider_id, esito = stato, "accesso del client finito");
        });
    }

    /// `esci`: il comando di uscita, poi l'account riletto.
    pub fn esci(
        &self,
        richiesta_id: String,
        provider_id: String,
        p: Result<Box<dyn Provider>, String>,
    ) {
        let me = self.clone();
        tokio::spawn(async move {
            let p = match p {
                Ok(p) => p,
                Err(e) => return me.errore(&richiesta_id, &provider_id, e),
            };
            let Some(cmd) = p.comando_uscita() else {
                return me.errore(
                    &richiesta_id,
                    &provider_id,
                    "questo client non ha un comando di uscita".into(),
                );
            };
            let argomenti: Vec<&str> = cmd.argomenti.iter().map(String::as_str).collect();
            match accesso::uscita(&cmd.programma, &argomenti).await {
                Ok((true, _)) => {}
                Ok((false, _)) => {
                    return me.errore(&richiesta_id, &provider_id, "il client non è uscito".into());
                }
                Err(e) => return me.errore(&richiesta_id, &provider_id, e.to_string()),
            }
            let account = crate::scoperta::leggi_account(p.as_ref()).await;
            let _ = me.cambi.send((provider_id.clone(), account.clone()));
            let e = Esito {
                stato: "uscito",
                account,
                ..Default::default()
            };
            me.manda(Some(&richiesta_id), &provider_id, e);
            tracing::info!(provider = %provider_id, "client uscito");
        });
    }

    /// `codice_accesso`: scrive il codice sullo stdin dell'accesso in corso.
    pub fn codice(&self, provider_id: String, codice: String) {
        let preso = self
            .stdin
            .lock()
            .ok()
            .and_then(|mut m| m.remove(&provider_id));
        let Some(mut s) = preso else {
            tracing::warn!(provider = %provider_id, "codice senza un accesso che lo aspetti");
            return;
        };
        let me = self.clone();
        tokio::spawn(async move {
            if let Err(e) = accesso::scrivi_codice(&mut s, &codice).await {
                tracing::warn!(provider = %provider_id, errore = %e, "codice non scritto");
            }
            if let Ok(mut m) = me.stdin.lock() {
                m.insert(provider_id, s);
            }
        });
    }
}
