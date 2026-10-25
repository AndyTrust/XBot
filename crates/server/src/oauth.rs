//! OAuth dei collegamenti (Fase 2.I) con `rmcp::transport::auth`: scoperta dei metadati dal
//! server MCP, registrazione dinamica (client pubblico, PKCE S256), scambio del codice, rinnovo
//! e revoca. I tentativi aperti stanno in memoria (`state` → gestore con il verificatore PKCE),
//! scadono in 10 minuti, al massimo 20 vivi. Mai un token nei log o in un URL.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use base64::Engine;
use rmcp::transport::CredentialStore;
use rmcp::transport::auth::{AuthorizationMetadata, OAuthClientConfig, OAuthTokenResponse};
use rmcp::transport::{AuthorizationManager, InMemoryCredentialStore, StoredCredentials};
use serde_json::{Value, json};
use xbot_core::{Codice, Errore, Ms, Risposta, SegretoTesto, ora_ms};
use xbot_db::cassaforte::Chiave;
use xbot_db::oauth::{self as repo, Accesso, Token};
use xbot_db::{Db, eventi};

/// Quanto vive un tentativo (10 minuti) e quanti ne restano aperti.
const DURATA_MS: Ms = 10 * 60 * 1000;
const MASSIMO: usize = 20;
/// Il token si rinnova se manca meno di questo alla scadenza.
const MARGINE_MS: Ms = 60 * 1000;
/// Il testo per l'utente quando il rinnovo fallisce.
pub const SCADUTO: &str = "l'accesso è scaduto: ricollega l'account";
pub const SERVE_ACCESSO: &str = "serve il collegamento dell'account";

/// Un tentativo aperto da `avvia`.
pub struct Tentativo {
    pub collegamento_id: String,
    gestore: AuthorizationManager,
    metadati: AuthorizationMetadata,
    url: String,
    client_id: String,
    client_secret: Option<SegretoTesto>,
    creato_ms: Ms,
}

/// I tentativi aperti (in `Stato`).
#[derive(Default)]
pub struct Tentativi(Mutex<HashMap<String, Tentativo>>);

impl Tentativi {
    fn mappa(&self) -> std::sync::MutexGuard<'_, HashMap<String, Tentativo>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn metti(&self, state: String, t: Tentativo) {
        let mut m = self.mappa();
        let ora = ora_ms();
        m.retain(|_, v| ora - v.creato_ms < DURATA_MS);
        while m.len() >= MASSIMO {
            let vecchio = m
                .iter()
                .min_by_key(|(_, v)| v.creato_ms)
                .map(|(k, _)| k.clone());
            match vecchio {
                Some(k) => m.remove(&k),
                None => break,
            };
        }
        // Un secondo `avvia` sullo stesso collegamento sostituisce il primo.
        m.retain(|_, v| v.collegamento_id != t.collegamento_id);
        m.insert(state, t);
    }

    fn prendi(&self, state: &str) -> Option<Tentativo> {
        let t = self.mappa().remove(state)?;
        (ora_ms() - t.creato_ms < DURATA_MS).then_some(t)
    }

    /// Il collegamento di un tentativo, senza consumarlo.
    pub fn collegamento_di(&self, state: &str) -> Option<String> {
        self.mappa().get(state).map(|t| t.collegamento_id.clone())
    }

    /// Chiude un tentativo (rifiuto del provider).
    pub fn togli(&self, state: &str) -> Option<String> {
        self.mappa().remove(state).map(|t| t.collegamento_id)
    }

    pub fn quanti(&self) -> usize {
        self.mappa().len()
    }
}

fn fallito(testo: impl std::fmt::Display) -> Errore {
    Errore::nuovo(Codice::OauthFallito, testo.to_string())
}

/// Il gestore di `rmcp` con i metadati scoperti dal server MCP.
async fn gestore_per(url: &str) -> Risposta<(AuthorizationManager, AuthorizationMetadata)> {
    let mut g = AuthorizationManager::new(url)
        .await
        .map_err(|e| fallito(format!("indirizzo non valido: {e}")))?;
    let r = g
        .resolve_metadata()
        .await
        .map_err(|e| fallito(format!("metadati OAuth non trovati: {e}")))?;
    if !r.source.is_discovered() {
        return Err(fallito("il server non dichiara un accesso OAuth"));
    }
    g.set_metadata(r.metadata.clone());
    Ok((g, r.metadata))
}

/// I dati del collegamento che servono qui: url e accesso salvato.
async fn collegamento(
    db: &Db,
    chiave: Option<std::sync::Arc<Chiave>>,
    id: &str,
) -> Risposta<(String, Option<Accesso>)> {
    let id = id.to_string();
    db.esegui(move |c| {
        let v = xbot_db::collegamenti::per_prova(c, chiave.as_deref(), &id)?;
        if v.trasporto != "http" {
            return Err(Errore::non_valida(
                "OAuth vale solo per i collegamenti http",
            ));
        }
        let url = v.url.ok_or_else(|| Errore::non_valida("manca l'url"))?;
        Ok((url, repo::leggi(c, chiave.as_deref(), &id)?))
    })
    .await
}

/// `POST /collegamenti/{id}/oauth/avvia`: l'URL di autorizzazione con `state` e PKCE.
pub async fn avvia(
    db: &Db,
    chiave: Option<std::sync::Arc<Chiave>>,
    tentativi: &Tentativi,
    id: &str,
    ritorno: &str,
) -> Risposta<String> {
    let (url, salvato) = collegamento(db, chiave.clone(), id).await?;
    let (mut g, metadati) = gestore_per(&url).await?;
    let scopi = g.select_initial_scopes(&[]).await;
    let scopi_ref: Vec<&str> = scopi.iter().map(String::as_str).collect();
    let riuso = salvato.and_then(|a| a.client_id.map(|c| (c, a.client_secret)));
    let (client_id, client_secret) = match riuso {
        Some((cid, segreto)) => {
            let mut cfg = OAuthClientConfig::new(cid.clone(), ritorno);
            if let Some(s) = &segreto {
                cfg = cfg.with_client_secret(s.esponi());
            }
            g.configure_client(cfg).map_err(fallito)?;
            (cid, segreto)
        }
        None => {
            let cfg = g
                .register_client("XBot", ritorno, &scopi_ref)
                .await
                .map_err(|e| fallito(format!("registrazione del client rifiutata: {e}")))?;
            (cfg.client_id, cfg.client_secret.map(SegretoTesto::from))
        }
    };
    let indirizzo = g
        .get_authorization_url(&scopi_ref)
        .await
        .map_err(|e| fallito(format!("URL di autorizzazione: {e}")))?;
    let state = reqwest::Url::parse(&indirizzo)
        .ok()
        .and_then(|u| {
            u.query_pairs()
                .find(|(k, _)| k == "state")
                .map(|(_, v)| v.into_owned())
        })
        .ok_or_else(|| fallito("URL di autorizzazione senza state"))?;
    let id2 = id.to_string();
    db.esegui(move |c| repo::segna_richiede(c, &id2, None))
        .await?;
    tentativi.metti(
        state,
        Tentativo {
            collegamento_id: id.to_string(),
            gestore: g,
            metadati,
            url,
            client_id,
            client_secret,
            creato_ms: ora_ms(),
        },
    );
    tracing::info!(collegamento = %id, "OAuth avviato");
    Ok(indirizzo)
}

/// Il token come lo salva XBot, dalla risposta del server di autorizzazione.
fn token_da(r: &OAuthTokenResponse, vecchio_refresh: Option<&str>) -> Risposta<Token> {
    let v = serde_json::to_value(r).map_err(|e| fallito(format!("token illeggibili: {e}")))?;
    let access_token = v["access_token"]
        .as_str()
        .ok_or_else(|| fallito("risposta senza access_token"))?
        .to_string();
    Ok(Token {
        access_token,
        refresh_token: v["refresh_token"]
            .as_str()
            .or(vecchio_refresh)
            .map(str::to_string),
        token_type: v["token_type"].as_str().unwrap_or("bearer").to_lowercase(),
        scope: v["scope"].as_str().map(str::to_string),
        expires_at_ms: v["expires_in"].as_i64().map(|s| ora_ms() + s * 1000),
    })
}

/// L'identità da mostrare: `userinfo_endpoint`, poi il JWT, poi l'host del server.
async fn identita(metadati: &AuthorizationMetadata, token: &Token, url: &str) -> String {
    let campi = |v: &Value| -> Option<String> {
        ["email", "preferred_username", "name", "sub"]
            .iter()
            .find_map(|k| v[*k].as_str().filter(|s| !s.is_empty()).map(str::to_string))
    };
    if let Some(ui) = metadati
        .additional_fields
        .get("userinfo_endpoint")
        .and_then(Value::as_str)
    {
        let r = reqwest::Client::new()
            .get(ui)
            .bearer_auth(&token.access_token)
            .timeout(Duration::from_secs(10))
            .send()
            .await;
        if let Ok(r) = r
            && r.status().is_success()
            && let Ok(v) = r.json::<Value>().await
            && let Some(n) = campi(&v)
        {
            return n;
        }
    }
    let parti: Vec<&str> = token.access_token.split('.').collect();
    if parti.len() == 3
        && let Ok(b) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parti[1])
        && let Ok(v) = serde_json::from_slice::<Value>(&b)
        && let Some(n) = campi(&v)
    {
        return n;
    }
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_else(|| "collegato".into())
}

/// `GET /oauth/ritorno`: scambia il codice e salva l'accesso. Restituisce l'id del collegamento.
pub async fn ritorno(
    db: &Db,
    chiave: Option<std::sync::Arc<Chiave>>,
    tentativi: &Tentativi,
    codice: &str,
    state: &str,
) -> Risposta<String> {
    let t = tentativi.prendi(state).ok_or_else(|| {
        Errore::nuovo(
            Codice::OauthStatoNonValido,
            "tentativo di collegamento sconosciuto o scaduto: ripeti «Collega»",
        )
    })?;
    let r = t
        .gestore
        .exchange_code_for_token(codice, state)
        .await
        .map_err(|e| fallito(format!("scambio del codice rifiutato: {e}")))?;
    let token = token_da(&r, None)?;
    let account = identita(&t.metadati, &token, &t.url).await;
    let accesso = Accesso {
        collegamento_id: t.collegamento_id.clone(),
        account: account.clone(),
        stato: "ok".into(),
        scopi: token.scope.clone(),
        token: Some(token),
        client_id: Some(t.client_id),
        client_secret: t.client_secret,
        issuer: t.metadati.issuer.clone(),
        scade_ms: None,
    };
    let id = t.collegamento_id;
    let id2 = id.clone();
    db.esegui(move |c| {
        repo::salva(c, chiave.as_deref(), &accesso)?;
        let nome = repo::nome_collegamento(c, &id2)?;
        eventi::scrivi(
            c,
            "oauth_collegato",
            &json!({ "collegamento_id": id2, "nome": nome, "account": account }),
        )
    })
    .await?;
    tracing::info!(collegamento = %id, "OAuth collegato");
    Ok(id)
}

/// Com'è andato un rinnovo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rinnovo {
    /// Non serviva (o non c'è un accesso `ok`).
    Niente,
    Rinnovato,
    /// Fallito: l'accesso ora è `scaduto`.
    Scaduto,
}

/// Rinnova il token se manca meno di un minuto (o sempre, con `forza`).
pub async fn rinnova(
    db: &Db,
    chiave: Option<std::sync::Arc<Chiave>>,
    id: &str,
    forza: bool,
) -> Risposta<Rinnovo> {
    let (url, salvato) = collegamento(db, chiave.clone(), id).await?;
    let Some(a) = salvato.filter(|a| a.stato == "ok") else {
        return Ok(Rinnovo::Niente);
    };
    let Some(t) = a.token.clone() else {
        return Ok(Rinnovo::Niente);
    };
    if !forza && t.expires_at_ms.is_none_or(|e| e - ora_ms() > MARGINE_MS) {
        return Ok(Rinnovo::Niente);
    }
    match rinnova_da(&url, &a, &t).await {
        Ok(nuovo) => {
            let id2 = id.to_string();
            db.esegui(move |c| repo::aggiorna_token(c, chiave.as_deref(), &id2, &nuovo))
                .await?;
            tracing::info!(collegamento = %id, "token OAuth rinnovato");
            Ok(Rinnovo::Rinnovato)
        }
        Err(e) => {
            tracing::warn!(collegamento = %id, errore = %e, "rinnovo OAuth fallito");
            let id2 = id.to_string();
            let account = a.account.clone();
            db.esegui(move |c| {
                repo::segna_scaduto(c, &id2, SCADUTO)?;
                let nome = repo::nome_collegamento(c, &id2)?;
                eventi::scrivi(
                    c,
                    "oauth_scaduto",
                    &json!({ "collegamento_id": id2, "nome": nome, "account": account }),
                )
            })
            .await?;
            Ok(Rinnovo::Scaduto)
        }
    }
}

async fn rinnova_da(url: &str, a: &Accesso, t: &Token) -> Risposta<Token> {
    let refresh = t
        .refresh_token
        .as_deref()
        .ok_or_else(|| fallito("manca il refresh_token"))?;
    let cid = a
        .client_id
        .clone()
        .ok_or_else(|| fallito("manca il client_id"))?;
    let (mut g, _) = gestore_per(url).await?;
    let mut cfg = OAuthClientConfig::new(cid.clone(), "http://localhost/api/v1/oauth/ritorno");
    if let Some(s) = &a.client_secret {
        cfg = cfg.with_client_secret(s.esponi());
    }
    g.configure_client(cfg).map_err(fallito)?;
    let resto = t
        .expires_at_ms
        .map_or(0, |e| ((e - ora_ms()) / 1000).max(0));
    let vecchio: OAuthTokenResponse = serde_json::from_value(json!({
        "access_token": t.access_token, "token_type": "bearer",
        "refresh_token": refresh, "expires_in": resto,
    }))
    .map_err(|e| fallito(format!("token salvati illeggibili: {e}")))?;
    let scopi: Vec<String> = a
        .scopi
        .as_deref()
        .map(|s| s.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();
    let secondi = u64::try_from(ora_ms() / 1000).unwrap_or(0);
    let deposito = InMemoryCredentialStore::new();
    deposito
        .save(StoredCredentials::new(
            cid,
            Some(vecchio),
            scopi,
            Some(secondi),
        ))
        .await
        .map_err(fallito)?;
    g.set_credential_store(deposito);
    let r = g
        .refresh_token()
        .await
        .map_err(|e| fallito(format!("rinnovo rifiutato: {e}")))?;
    token_da(&r, Some(refresh))
}

/// Rinnova gli accessi dei collegamenti accesi di un assistente prima del turno. Restituisce i
/// nomi di quelli che restano fuori (accesso scaduto o mai dato) e gli id appena scaduti.
pub async fn prepara_turno(
    db: &Db,
    chiave: Option<std::sync::Arc<Chiave>>,
    assistente_id: &str,
) -> (Vec<String>, Vec<String>) {
    let aid = assistente_id.to_string();
    let ids = db
        .esegui(move |c| repo::ok_per_assistente(c, &aid))
        .await
        .unwrap_or_default();
    let mut scaduti = Vec::new();
    for id in ids {
        match rinnova(db, chiave.clone(), &id, false).await {
            Ok(Rinnovo::Scaduto) => scaduti.push(id),
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(collegamento = %id, errore = %e, "rinnovo prima del turno non riuscito")
            }
        }
    }
    let aid = assistente_id.to_string();
    let fuori = db
        .esegui(move |c| repo::senza_accesso_per_assistente(c, &aid))
        .await
        .unwrap_or_default();
    (fuori, scaduti)
}

/// `DELETE /collegamenti/{id}/oauth`: revoca (se il server la offre), token cancellati.
pub async fn scollega(db: &Db, chiave: Option<std::sync::Arc<Chiave>>, id: &str) -> Risposta<()> {
    let (url, salvato) = collegamento(db, chiave, id).await?;
    let Some(a) = salvato.filter(|a| a.stato != "revocato") else {
        return Err(Errore::nuovo(
            Codice::NonTrovato,
            "questo collegamento non ha un account collegato",
        ));
    };
    if let Some(t) = &a.token {
        revoca(&url, &a, t).await;
    }
    let id2 = id.to_string();
    let account = a.account;
    db.esegui(move |c| {
        repo::segna(c, &id2, "revocato")?;
        let nome = repo::nome_collegamento(c, &id2)?;
        eventi::scrivi(
            c,
            "oauth_scollegato",
            &json!({ "collegamento_id": id2, "nome": nome, "account": account }),
        )
    })
    .await?;
    tracing::info!(collegamento = %id, "OAuth scollegato");
    Ok(())
}

/// RFC 7009: errori ignorati (solo nel log).
async fn revoca(url: &str, a: &Accesso, t: &Token) {
    let punto = match gestore_per(url).await {
        Ok((_, m)) => m
            .additional_fields
            .get("revocation_endpoint")
            .and_then(Value::as_str)
            .map(str::to_string),
        Err(e) => {
            tracing::warn!(errore = %e, "revoca: metadati non letti");
            None
        }
    };
    let Some(punto) = punto else { return };
    let (token, tipo) = match &t.refresh_token {
        Some(r) => (r.as_str(), "refresh_token"),
        None => (t.access_token.as_str(), "access_token"),
    };
    let mut modulo = vec![("token", token), ("token_type_hint", tipo)];
    if let Some(c) = a.client_id.as_deref() {
        modulo.push(("client_id", c));
    }
    let r = reqwest::Client::new()
        .post(&punto)
        .form(&modulo)
        .timeout(Duration::from_secs(10))
        .send()
        .await;
    match r {
        Ok(r) if r.status().is_success() => tracing::info!("token OAuth revocato"),
        Ok(r) => tracing::warn!(stato = %r.status(), "revoca OAuth rifiutata"),
        Err(e) => tracing::warn!(errore = %e, "revoca OAuth non arrivata"),
    }
}

/// Un errore di prova che dice «serve l'accesso» (401 dal server MCP).
pub fn chiede_accesso(errore: &str) -> bool {
    let e = errore.to_lowercase();
    e.contains("auth required") || e.contains("authorization required") || e.contains("401")
}
