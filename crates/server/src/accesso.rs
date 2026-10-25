//! Accesso: impianto dell'utente, password (Argon2id) e TOTP, cookie firmato, dispositivi
//! fidati (cookie `xbot_fidato`, migrazione 008), guardia delle rotte.

use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use axum::Json;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum_extra::extract::cookie::{Cookie, SameSite, SignedCookieJar};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use totp_rs::{Builder, Secret, Totp};
use xbot_core::{Codice, Errore, Risposta, SegretoTesto, ora_ms};
use xbot_db::cassaforte::{self, Chiave};
use xbot_db::{Connessione, Db, accesso as repo, impostazioni};

use crate::cliente::IpCliente;
use crate::errore_http::{Corpo, ErroreHttp};
use crate::stato::Stato;

pub const COOKIE: &str = "xbot_accesso";
const CHIAVE_PASSWORD: &str = "accesso.password_hash";
/// L'ultimo passo TOTP accettato (contro il riuso del codice).
const CHIAVE_PASSO: &str = "accesso.totp_ultimo_passo";
const ID_TOTP: &str = "utente:totp";

/// Ricifra il segreto TOTP con una chiave nuova (rotazione di `XBOT_SEGRETO_COOKIE`).
/// Dà `Ok(false)` se l'utente non ha il TOTP.
pub fn ricifra_totp(db: &Db, vecchia: &Chiave, nuova: &Chiave) -> Risposta<bool> {
    db.con(|c| {
        let Some((nome, Some(cifrato))) = repo::utente(c)? else {
            return Ok(false);
        };
        let chiaro = cassaforte::decifra(vecchia, ID_TOTP, &cifrato)?;
        let nuovo = cassaforte::cifra(nuova, ID_TOTP, &chiaro)?;
        repo::imposta_utente(c, &nome, Some(&nuovo))?;
        Ok(true)
    })
}
/// Il massimo che Chrome concede a un cookie (400 giorni). Il dispositivo fidato si rinnova a ogni uso,
/// quindi il codice si chiede una volta sola, finché l'utente non lo revoca.
const DURATA_MS: i64 = 400 * 24 * 3600 * 1000;

fn negato() -> Errore {
    Errore::nuovo(Codice::AccessoNegato, "password o codice sbagliati")
}

pub fn hash_password(password: &SegretoTesto) -> Risposta<String> {
    Argon2::default()
        .hash_password(password.esponi().as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| Errore::interno(format!("hash della password: {e}")))
}

fn password_giusta(conn: &Connessione, password: &SegretoTesto) -> Risposta<bool> {
    let Some(v) = impostazioni::leggi(conn, CHIAVE_PASSWORD)? else {
        return Ok(false);
    };
    let Some(hash) = v.as_str() else {
        return Ok(false);
    };
    let hash =
        PasswordHash::new(hash).map_err(|e| Errore::interno(format!("hash salvato: {e}")))?;
    Ok(Argon2::default()
        .verify_password(password.esponi().as_bytes(), &hash)
        .is_ok())
}

fn costruisci_totp(segreto: Vec<u8>, nome: &str) -> Risposta<Totp> {
    Builder::new()
        .with_secret(segreto)
        .with_account_name(nome)
        .with_issuer(Some("XBot"))
        .build()
        .map_err(|e| Errore::interno(format!("TOTP: {e}")))
}

/// Il TOTP dell'utente, se impostato.
fn totp_utente(conn: &Connessione, chiave: &Chiave) -> Risposta<Option<Totp>> {
    let Some((nome, Some(cifrato))) = repo::utente(conn)? else {
        return Ok(None);
    };
    let segreto = cassaforte::decifra(chiave, ID_TOTP, &cifrato)?;
    costruisci_totp(segreto, &nome).map(Some)
}

/// Crea (o rifà) l'utente. Con `con_totp` genera il segreto e restituisce l'URL `otpauth://`.
pub fn impianta(
    db: &Db,
    chiave_totp: &Chiave,
    nome: &str,
    password: &SegretoTesto,
    con_totp: bool,
) -> Risposta<Option<String>> {
    if nome.trim().is_empty() {
        return Err(Errore::non_valida("il nome è obbligatorio"));
    }
    if password.lunghezza() < 8 {
        return Err(Errore::non_valida("la password vuole almeno 8 caratteri"));
    }
    let hash = hash_password(password)?;
    let (cifrato, url) = if con_totp {
        let segreto = Secret::generate().as_bytes().to_vec();
        let totp = costruisci_totp(segreto.clone(), nome.trim())?;
        let url = totp
            .to_url()
            .map_err(|e| Errore::interno(format!("URL TOTP: {e}")))?;
        (
            Some(cassaforte::cifra(chiave_totp, ID_TOTP, &segreto)?),
            Some(url),
        )
    } else {
        (None, None)
    };
    db.con(|c| {
        xbot_db::in_transazione(c, || {
            impostazioni::scrivi(c, CHIAVE_PASSWORD, &serde_json::json!(hash))?;
            repo::imposta_utente(c, nome.trim(), cifrato.as_deref())
        })
    })?;
    Ok(url)
}

/// Un codice TOTP vale una volta sola: il passo usato deve superare l'ultimo ricordato.
fn usa_passo(conn: &Connessione, passo: u64) -> Risposta<()> {
    let ultimo = impostazioni::leggi(conn, CHIAVE_PASSO)?.and_then(|v| v.as_u64());
    if ultimo.is_some_and(|u| passo <= u) {
        tracing::warn!("codice TOTP già usato: rifiutato");
        return Err(negato());
    }
    impostazioni::scrivi(conn, CHIAVE_PASSO, &serde_json::json!(passo))
}

/// Hash del token per la tabella `accesso_web` (mai il token in chiaro).
pub fn hash_token(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[derive(Deserialize)]
pub struct Entra {
    pub password: SegretoTesto,
    pub codice_totp: Option<SegretoTesto>,
    pub dispositivo: Option<String>,
    /// Con `true` e un accesso completo (password + TOTP) si rilascia il cookie `xbot_fidato`.
    #[serde(default)]
    pub fidato: bool,
}

/// Il cookie del dispositivo fidato: con questo basta la password.
pub const COOKIE_FIDATO: &str = "xbot_fidato";

/// Il cookie che cancella `xbot_fidato` nella risposta.
fn via_fidato() -> Cookie<'static> {
    Cookie::build((COOKIE_FIDATO, "")).path("/").build()
}

/// L'hash del cookie fidato della richiesta, se firmato correttamente (altrimenti si ignora).
fn hash_fidato(jar: &SignedCookieJar) -> Option<String> {
    jar.get(COOKIE_FIDATO).map(|c| hash_token(c.value()))
}

/// Come è andato il controllo dentro la transazione.
struct Esito {
    /// Id del dispositivo fidato usato o creato.
    fidato_id: Option<String>,
    /// Il cookie fidato della richiesta non vale più (scaduto, revocato, sconosciuto).
    fidato_vecchio: bool,
    /// Il valore del nuovo cookie fidato da rilasciare.
    nuovo_fidato: Option<String>,
}

/// `POST /accesso`: password e TOTP giusti → cookie `xbot_accesso`. Con il cookie `xbot_fidato`
/// valido basta la password; con `fidato: true` e un accesso completo si rilascia `xbot_fidato`.
pub async fn entra(
    State(s): State<Stato>,
    ip: IpCliente,
    intestazioni: HeaderMap,
    jar: SignedCookieJar,
    Corpo(e): Corpo<Entra>,
) -> Response {
    if s.tentativi.bloccato(ip.0) {
        tracing::warn!(ip = %ip, "accesso rifiutato: troppi tentativi falliti");
        return ErroreHttp(Errore::nuovo(
            Codice::TroppiTentativi,
            "troppi tentativi falliti: riprova fra qualche minuto",
        ))
        .into_response();
    }
    let token = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
    let id_hash = hash_token(&token);
    let dispositivo = e
        .dispositivo
        .clone()
        .filter(|d| !d.trim().is_empty())
        .unwrap_or_else(|| "sconosciuto".into());
    let agente = intestazioni
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.chars().take(300).collect::<String>());
    let hash_cookie_fidato = hash_fidato(&jar);
    let valore_fidato = jar.get(COOKIE_FIDATO).map(|c| c.value().to_string());
    // solo i nomi dei cookie arrivati, mai i valori: serve a capire perché un dispositivo viene richiesto di nuovo
    let nomi_cookie: Vec<String> = intestazioni
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            v.split(';')
                .filter_map(|c| c.split('=').next())
                .map(|n| n.trim().to_string())
                .collect()
        })
        .unwrap_or_default();
    let ha_cookie_fidato = hash_cookie_fidato.is_some();
    let scade_ms = ora_ms() + DURATA_MS;
    let stato = s.clone();
    let disp = dispositivo.clone();
    let esito =
        s.db.esegui(move |c| {
            if repo::utente(c)?.is_none() || !password_giusta(c, &e.password)? {
                return Err(negato());
            }
            let mut esito = Esito {
                fidato_id: None,
                fidato_vecchio: false,
                nuovo_fidato: None,
            };
            if let Some(h) = &hash_cookie_fidato {
                esito.fidato_id = repo::fidato_usa(c, h, ora_ms() + DURATA_MS)?;
                esito.fidato_vecchio = esito.fidato_id.is_none();
            }
            if esito.fidato_id.is_none()
                && let Some(totp) = totp_utente(c, &stato.chiave_totp)?
            {
                let codice = e
                    .codice_totp
                    .as_ref()
                    .map(|c| c.esponi().trim().to_string())
                    .filter(|c| !c.is_empty());
                let Some(codice) = codice else {
                    return Err(Errore::nuovo(
                        Codice::TotpRichiesto,
                        "serve il codice TOTP: questo dispositivo non è fidato",
                    ));
                };
                let Some(passo) = totp.check_current(&codice) else {
                    return Err(negato());
                };
                usa_passo(c, passo)?;
                if e.fidato {
                    let valore = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
                    let id = xbot_core::nuovo_id();
                    repo::fidato_crea(
                        c,
                        &id,
                        &disp,
                        &hash_token(&valore),
                        ora_ms() + DURATA_MS,
                        agente.as_deref(),
                    )?;
                    esito.fidato_id = Some(id);
                    esito.nuovo_fidato = Some(valore);
                }
            }
            repo::crea(c, &id_hash, &disp, scade_ms)?;
            Ok(esito)
        })
        .await;
    let jar = match &esito {
        Ok(es) if es.fidato_vecchio && es.nuovo_fidato.is_none() => jar.remove(via_fidato()),
        Err(_) if ha_cookie_fidato => {
            // Il cookie fidato resta solo se è ancora valido: lo si riguarda fuori dal passo fallito.
            let h = hash_fidato(&jar).unwrap_or_default();
            match s.db.esegui(move |c| repo::fidato_id(c, &h)).await {
                Ok(None) => jar.remove(via_fidato()),
                _ => jar,
            }
        }
        _ => jar,
    };
    let esito = match esito {
        Ok(es) => {
            s.tentativi.riuscito(ip.0);
            es
        }
        Err(err) => {
            if matches!(err.codice, Codice::AccessoNegato | Codice::TotpRichiesto) {
                tracing::warn!(ip = %ip, dispositivo = %dispositivo, codice = err.codice.come_testo(), cookie = ?nomi_cookie, "accesso negato");
                s.tentativi.fallito(ip.0);
            }
            return (jar, ErroreHttp(err)).into_response();
        }
    };
    if let (Some(id), None) = (&esito.fidato_id, &esito.nuovo_fidato) {
        tracing::info!(ip = %ip, "accesso con dispositivo fidato id={id}");
    }
    tracing::info!(ip = %ip, dispositivo = %dispositivo, "accesso aperto");
    let cookie = Cookie::build((COOKIE, token))
        .http_only(true)
        .same_site(SameSite::Lax)
        .secure(s.cookie_sicuro)
        .path("/")
        .max_age(time_durata(DURATA_MS))
        .build();
    let mut jar = jar.add(cookie);
    let appena_creato = esito.nuovo_fidato.is_some();
    if let (Some(valore), Some(id)) = (esito.nuovo_fidato, &esito.fidato_id) {
        tracing::info!(ip = %ip, "dispositivo fidato creato id={id}");
        jar = jar.add(
            Cookie::build((COOKIE_FIDATO, valore))
                .http_only(true)
                .same_site(SameSite::Lax)
                .secure(s.cookie_sicuro)
                .path("/")
                .max_age(time_durata(DURATA_MS))
                .build(),
        );
    }
    // un dispositivo fidato già noto: il cookie si rinnova a ogni accesso (resta valido finché si usa)
    if let (false, Some(valore), Some(_)) = (appena_creato, valore_fidato, &esito.fidato_id) {
        jar = jar.add(
            Cookie::build((COOKIE_FIDATO, valore))
                .http_only(true)
                .same_site(SameSite::Lax)
                .secure(s.cookie_sicuro)
                .path("/")
                .max_age(time_durata(DURATA_MS))
                .build(),
        );
    }
    (
        jar,
        Json(serde_json::json!({
            "ok": true,
            "dispositivo": dispositivo,
            "scade_ms": scade_ms,
            "fidato": esito.fidato_id.is_some(),
            "dispositivo_fidato_id": esito.fidato_id,
        })),
    )
        .into_response()
}

/// `GET /dispositivi-fidati`: i dispositivi fidati validi; `corrente` = quello del cookie.
pub async fn fidati_elenco(
    State(s): State<Stato>,
    jar: SignedCookieJar,
) -> Result<impl IntoResponse, ErroreHttp> {
    let h = hash_fidato(&jar);
    let v =
        s.db.esegui(move |c| {
            let corrente = match &h {
                Some(h) => repo::fidato_id(c, h)?,
                None => None,
            };
            let elenco: Vec<_> = repo::fidati(c)?
                .into_iter()
                .map(|d| {
                    let mut v = serde_json::to_value(&d).unwrap_or_default();
                    v["corrente"] = serde_json::json!(corrente.as_deref() == Some(d.id.as_str()));
                    v
                })
                .collect();
            Ok(serde_json::json!({ "dispositivi": elenco }))
        })
        .await?;
    Ok(Json(v))
}

/// `DELETE /dispositivi-fidati/{id}`: revoca; se è il corrente il cookie si cancella.
pub async fn fidato_revoca(
    State(s): State<Stato>,
    jar: SignedCookieJar,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ErroreHttp> {
    let h = hash_fidato(&jar);
    let id2 = id.clone();
    let (trovato, era_corrente) =
        s.db.esegui(move |c| {
            let corrente = match &h {
                Some(h) => repo::fidato_id(c, h)?,
                None => None,
            };
            Ok((
                repo::fidato_revoca(c, &id2)?,
                corrente.as_deref() == Some(id2.as_str()),
            ))
        })
        .await?;
    if !trovato {
        return Err(Errore::nuovo(Codice::NonTrovato, "dispositivo fidato non trovato").into());
    }
    tracing::info!("dispositivo fidato revocato id={id}");
    let jar = if era_corrente {
        jar.remove(via_fidato())
    } else {
        jar
    };
    Ok((jar, StatusCode::NO_CONTENT))
}

/// `DELETE /dispositivi-fidati`: revoca tutti; il cookie della richiesta si cancella.
pub async fn fidati_revoca_tutti(
    State(s): State<Stato>,
    jar: SignedCookieJar,
) -> Result<impl IntoResponse, ErroreHttp> {
    let n = s.db.esegui(repo::fidati_revoca_tutti).await?;
    tracing::info!(revocati = n, "dispositivi fidati revocati tutti");
    let jar = if jar.get(COOKIE_FIDATO).is_some() {
        jar.remove(via_fidato())
    } else {
        jar
    };
    Ok((jar, Json(serde_json::json!({ "revocati": n }))))
}

fn time_durata(ms: i64) -> time::Duration {
    time::Duration::milliseconds(ms)
}

/// `DELETE /accesso`: revoca l'accesso del cookie (se c'è) e lo toglie.
pub async fn esci(
    State(s): State<Stato>,
    jar: SignedCookieJar,
) -> Result<impl IntoResponse, ErroreHttp> {
    if let Some(c) = jar.get(COOKIE) {
        let id_hash = hash_token(c.value());
        s.db.esegui(move |c| repo::revoca(c, &id_hash)).await?;
    }
    let via = Cookie::build((COOKIE, "")).path("/").build();
    Ok((jar.remove(via), Json(serde_json::json!({ "ok": true }))))
}

/// Guardia delle rotte protette: cookie firmato e accesso valido nel database.
pub async fn richiede_accesso(
    State(s): State<Stato>,
    jar: SignedCookieJar,
    mut req: Request,
    next: Next,
) -> Response {
    let Some(c) = jar.get(COOKIE) else {
        return ErroreHttp(Errore::nuovo(Codice::NonAutorizzato, "serve l'accesso"))
            .into_response();
    };
    let id_hash = hash_token(c.value());
    match s.db.esegui(move |c| repo::valido(c, &id_hash)).await {
        Ok(Some(a)) => {
            req.extensions_mut().insert(a);
            next.run(req).await
        }
        Ok(None) => ErroreHttp(Errore::nuovo(
            Codice::NonAutorizzato,
            "accesso scaduto o revocato",
        ))
        .into_response(),
        Err(e) => ErroreHttp(e).into_response(),
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn impianto_password_e_totp() {
        let db = Db::apri(":memory:").unwrap();
        let k = Chiave::da_byte([3u8; 32]);
        let pw = SegretoTesto::nuovo("una-password-lunga");
        let url = impianta(&db, &k, "Mario", &pw, true).unwrap().unwrap();
        assert!(url.starts_with("otpauth://totp/"));
        db.con(|c| {
            assert!(password_giusta(c, &pw)?);
            assert!(!password_giusta(c, &SegretoTesto::nuovo("sbagliata!!"))?);
            let t = totp_utente(c, &k)?.unwrap();
            let codice = t.generate_current().to_string();
            assert!(t.check_current(&codice).is_some());
            Ok(())
        })
        .unwrap();
        assert!(impianta(&db, &k, "Mario", &SegretoTesto::nuovo("corta"), false).is_err());
        assert_eq!(hash_token("a").len(), 64);
    }
}
