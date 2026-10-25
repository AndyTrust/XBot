//! Notifiche Web Push (VAPID, RFC 8030/8291/8292) con `web-push-native` e `reqwest`.
//!
//! Il notificatore guarda il database ogni 3 secondi e manda una notifica a ogni abbonamento
//! quando: un turno finisce riuscito («tocca a te»), si apre una richiesta, compare un avviso.
//! Ogni notifica resta anche nella tabella `notifica` (titolo ≤ 200, corpo ≤ 140 caratteri).
//! Il carico JSON per `sw.js`: `{titolo, corpo, tipo, url, sessione_id, assistente_id}`.

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rusqlite::params;
use serde_json::{Value, json};
use web_push_native::jwt_simple::algorithms::ES256KeyPair;
use web_push_native::p256::elliptic_curve::sec1::ToEncodedPoint;
use web_push_native::p256::{PublicKey, SecretKey};
use web_push_native::{Auth, WebPushBuilder};
use xbot_core::{Errore, Risposta, SegretoTesto, nuovo_id, ora_ms};
use xbot_db::tutte_json;

use crate::Stato;

/// Le chiavi VAPID del server.
pub struct Vapid {
    coppia: ES256KeyPair,
    /// La chiave pubblica non compressa in base64url (per `pushManager.subscribe`).
    pub pubblica: String,
}

impl Vapid {
    /// Dalla chiave privata grezza (32 byte) in base64url.
    pub fn da_privata(privata: &SegretoTesto) -> Risposta<Self> {
        let byte = URL_SAFE_NO_PAD
            .decode(privata.esponi().trim().trim_end_matches('='))
            .map_err(|_| Errore::non_valida("XBOT_VAPID_PRIVATA: base64url non valido"))?;
        let segreta = SecretKey::from_slice(&byte)
            .map_err(|_| Errore::non_valida("XBOT_VAPID_PRIVATA: chiave P-256 non valida"))?;
        let pubblica =
            URL_SAFE_NO_PAD.encode(segreta.public_key().to_encoded_point(false).as_bytes());
        let coppia = ES256KeyPair::from_bytes(&byte)
            .map_err(|_| Errore::non_valida("XBOT_VAPID_PRIVATA: chiave non valida"))?;
        Ok(Self { coppia, pubblica })
    }
}

/// Una coppia nuova: (privata, pubblica) in base64url, per il file d'ambiente.
pub fn genera_chiavi() -> (String, String) {
    let coppia = ES256KeyPair::generate();
    let privata = URL_SAFE_NO_PAD.encode(coppia.to_bytes());
    let pubblica = SecretKey::from_slice(&coppia.to_bytes())
        .map(|s| URL_SAFE_NO_PAD.encode(s.public_key().to_encoded_point(false).as_bytes()))
        .unwrap_or_default();
    (privata, pubblica)
}

fn taglia(t: &str, max: usize) -> String {
    let t = t.trim();
    if t.chars().count() <= max {
        t.to_string()
    } else {
        let mut s: String = t.chars().take(max - 1).collect();
        s.push('…');
        s
    }
}

/// Una notifica da mandare.
#[derive(Debug, Clone)]
pub struct Notifica {
    pub tipo: &'static str,
    pub titolo: String,
    pub corpo: String,
    pub sessione_id: Option<String>,
    pub assistente_id: Option<String>,
}

/// Il richiamo HTTP: separato per poterlo sostituire nelle prove.
async fn manda(
    cliente: &reqwest::Client,
    req: axum::http::Request<Vec<u8>>,
) -> Result<u16, String> {
    let (parti, corpo) = req.into_parts();
    let mut r = cliente.post(parti.uri.to_string()).body(corpo);
    for (k, v) in &parti.headers {
        r = r.header(k.as_str(), v.as_bytes());
    }
    r.send()
        .await
        .map(|x| x.status().as_u16())
        .map_err(|e| e.to_string())
}

/// Salva la notifica e la manda a tutti gli abbonamenti. Gli abbonamenti scaduti (404/410)
/// si cancellano. Senza VAPID salva soltanto.
pub async fn notifica(s: &Stato, cliente: &reqwest::Client, n: Notifica) -> Risposta<usize> {
    let titolo = taglia(&n.titolo, 200);
    let corpo = taglia(&n.corpo, 140);
    let url = n
        .sessione_id
        .as_ref()
        .map_or_else(|| "/".to_string(), |id| format!("/?sessione={id}"));
    let carico = json!({
        "titolo": titolo, "corpo": corpo, "tipo": n.tipo, "url": url,
        "sessione_id": n.sessione_id, "assistente_id": n.assistente_id,
    });
    let (t2, c2, sid, aid) = (
        titolo.clone(),
        corpo.clone(),
        n.sessione_id.clone(),
        n.assistente_id.clone(),
    );
    let abbonamenti = s
        .db
        .esegui(move |c| {
            c.execute(
                "INSERT INTO notifica (id, assistente_id, sessione_id, titolo, corpo, percorso, creata_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![nuovo_id(), aid, sid, t2, c2, url, ora_ms()],
            )
            .map_err(|e| Errore::database(e.to_string()))?;
            tutte_json(c, "SELECT id, endpoint, chiavi_json FROM abbonamento_push", [])
        })
        .await?;
    let Some(vapid) = s.servizi.vapid.clone() else {
        return Ok(0);
    };
    let contatto = s
        .servizi
        .url_pubblico
        .clone()
        .unwrap_or_else(|| "mailto:xbot@localhost".into());
    let mut mandate = 0;
    for a in abbonamenti {
        let id = a["id"].as_str().unwrap_or("").to_string();
        let req = (|| {
            let endpoint = a["endpoint"].as_str()?.parse().ok()?;
            let p256dh = URL_SAFE_NO_PAD
                .decode(a["chiavi_json"]["p256dh"].as_str()?.trim_end_matches('='))
                .ok()?;
            let auth = URL_SAFE_NO_PAD
                .decode(a["chiavi_json"]["auth"].as_str()?.trim_end_matches('='))
                .ok()?;
            if auth.len() != 16 {
                return None;
            }
            WebPushBuilder::new(
                endpoint,
                PublicKey::from_sec1_bytes(&p256dh).ok()?,
                Auth::clone_from_slice(&auth),
            )
            .with_valid_duration(Duration::from_secs(3600))
            .with_vapid(&vapid.coppia, &contatto)
            .build(carico.to_string())
            .ok()
        })();
        let Some(req) = req else {
            tracing::warn!(abbonamento = %id, "abbonamento push illeggibile");
            continue;
        };
        let esito = manda(cliente, req).await;
        let sql = match esito {
            Ok(200..=299) => {
                mandate += 1;
                "UPDATE abbonamento_push SET ultimo_ok_ms = ?2 WHERE id = ?1"
            }
            Ok(404 | 410) => "DELETE FROM abbonamento_push WHERE id = ?1 AND ?2 IS NOT NULL",
            altro => {
                tracing::warn!(abbonamento = %id, esito = ?altro, "notifica push non consegnata");
                continue;
            }
        };
        s.db.esegui(move |c| {
            c.execute(sql, params![id, ora_ms()])
                .map_err(|e| Errore::database(e.to_string()))
        })
        .await?;
    }
    Ok(mandate)
}

/// Le notifiche nate fra `da` (escluso) e `a` (compreso): turni riusciti, richieste, avvisi.
/// Gli assistenti con `stato_lettura.notifiche = 0` restano zitti.
pub fn da_mandare(c: &xbot_db::Connessione, da: i64, a: i64) -> Risposta<Vec<Notifica>> {
    let comune =
        "JOIN sessione s ON s.id = x.sessione_id JOIN assistente a ON a.id = s.assistente_id
        LEFT JOIN stato_lettura sl ON sl.assistente_id = a.id";
    let filtro = "COALESCE(sl.notifiche, 1) = 1";
    let mut v = Vec::new();
    for r in tutte_json(
        c,
        &format!(
            "SELECT x.sessione_id, a.id AS assistente_id, a.nome,
               (SELECT json_extract(r.corpo_json, '$.testo') FROM riga r WHERE r.sessione_id = x.sessione_id
                  AND r.genere = 'messaggio' AND r.ruolo = 'assistente' ORDER BY r.seq DESC LIMIT 1) AS testo
             FROM turno x {comune} WHERE x.stato = 'riuscito' AND x.finito_ms > ?1 AND x.finito_ms <= ?2 AND {filtro}"
        ),
        params![da, a],
    )? {
        v.push(Notifica {
            tipo: "tocca_a_te",
            titolo: format!("{}: tocca a te", r["nome"].as_str().unwrap_or("XBot")),
            corpo: xbot_db::barra::anteprima(r["testo"].as_str().unwrap_or("Risposta pronta")),
            sessione_id: r["sessione_id"].as_str().map(str::to_string),
            assistente_id: r["assistente_id"].as_str().map(str::to_string),
        });
    }
    for r in tutte_json(
        c,
        &format!(
            "SELECT x.sessione_id, a.id AS assistente_id, a.nome, x.tipo, x.carico_json
             FROM richiesta x {comune} WHERE x.creata_ms > ?1 AND x.creata_ms <= ?2 AND {filtro}"
        ),
        params![da, a],
    )? {
        let k = &r["carico_json"];
        let corpo = ["descrizione", "comando", "motivo", "domanda"]
            .iter()
            .find_map(|c| k[*c].as_str())
            .map_or_else(
                || format!("Richiesta: {}", r["tipo"].as_str().unwrap_or("")),
                str::to_string,
            );
        v.push(Notifica {
            tipo: "richiesta",
            titolo: format!(
                "{} chiede la tua risposta",
                r["nome"].as_str().unwrap_or("Un assistente")
            ),
            corpo,
            sessione_id: r["sessione_id"].as_str().map(str::to_string),
            assistente_id: r["assistente_id"].as_str().map(str::to_string),
        });
    }
    for r in tutte_json(
        c,
        &format!(
            "SELECT x.sessione_id, a.id AS assistente_id, a.nome, x.corpo_json
             FROM riga x {comune} WHERE x.genere = 'avviso'
               AND COALESCE(json_extract(x.corpo_json, '$.codice'), '') != 'in_attesa_macchina'
               AND x.creata_ms > ?1 AND x.creata_ms <= ?2 AND {filtro}"
        ),
        params![da, a],
    )? {
        let k = &r["corpo_json"];
        v.push(Notifica {
            tipo: "avviso",
            titolo: format!("{}: avviso", r["nome"].as_str().unwrap_or("XBot")),
            corpo: k["testo"]
                .as_str()
                .or(k["codice"].as_str())
                .unwrap_or("avviso")
                .to_string(),
            sessione_id: r["sessione_id"].as_str().map(str::to_string),
            assistente_id: r["assistente_id"].as_str().map(str::to_string),
        });
    }
    Ok(v)
}

/// Il giro del notificatore: dal segno `da` a un secondo fa. Restituisce il nuovo segno.
pub async fn giro(s: &Stato, cliente: &reqwest::Client, da: i64) -> i64 {
    let a = ora_ms() - 1000;
    if a <= da {
        return da;
    }
    match s.db.esegui(move |c| da_mandare(c, da, a)).await {
        Ok(v) => {
            for n in v {
                if let Err(e) = notifica(s, cliente, n).await {
                    tracing::warn!(errore = %e, "notifica non mandata");
                }
            }
            a
        }
        Err(e) => {
            tracing::warn!(errore = %e, "notificatore: lettura fallita");
            da
        }
    }
}

pub fn avvia_notificatore(s: Stato) {
    tokio::spawn(async move {
        let cliente = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        let mut segno = ora_ms();
        let mut t = tokio::time::interval(Duration::from_secs(3));
        loop {
            t.tick().await;
            segno = giro(&s, &cliente, segno).await;
        }
    });
}

/// L'abbonamento come lo manda il browser: `{endpoint, chiavi{p256dh, auth}, dispositivo}`.
pub fn controlla_abbonamento(v: &Value) -> Risposta<(String, Value, Option<String>)> {
    let endpoint = v["endpoint"]
        .as_str()
        .filter(|e| e.starts_with("https://"))
        .ok_or_else(|| Errore::non_valida("endpoint https obbligatorio"))?;
    let chiavi = if v["chiavi"].is_object() {
        &v["chiavi"]
    } else {
        &v["keys"]
    };
    let (Some(p), Some(a)) = (chiavi["p256dh"].as_str(), chiavi["auth"].as_str()) else {
        return Err(Errore::non_valida(
            "chiavi.p256dh e chiavi.auth obbligatorie",
        ));
    };
    let ok = URL_SAFE_NO_PAD
        .decode(p.trim_end_matches('='))
        .ok()
        .and_then(|b| PublicKey::from_sec1_bytes(&b).ok())
        .is_some()
        && URL_SAFE_NO_PAD
            .decode(a.trim_end_matches('='))
            .is_ok_and(|b| b.len() == 16);
    if !ok {
        return Err(Errore::non_valida("chiavi dell'abbonamento non valide"));
    }
    Ok((
        endpoint.to_string(),
        json!({ "p256dh": p, "auth": a }),
        v["dispositivo"].as_str().map(|d| taglia(d, 80)),
    ))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn chiavi_e_abbonamento() {
        let (privata, pubblica) = genera_chiavi();
        let v = Vapid::da_privata(&SegretoTesto::nuovo(privata)).unwrap();
        assert_eq!(v.pubblica, pubblica);
        assert_eq!(URL_SAFE_NO_PAD.decode(&pubblica).unwrap().len(), 65);
        assert!(Vapid::da_privata(&SegretoTesto::nuovo("x")).is_err());
        let ua = SecretKey::from_slice(&[7u8; 32]).unwrap();
        let p256dh = URL_SAFE_NO_PAD.encode(ua.public_key().to_encoded_point(false).as_bytes());
        let ab = json!({"endpoint": "https://push.example/x", "chiavi": {"p256dh": p256dh, "auth": URL_SAFE_NO_PAD.encode([1u8; 16])}});
        assert!(controlla_abbonamento(&ab).is_ok());
        assert!(controlla_abbonamento(&json!({"endpoint": "http://x"})).is_err());
        assert_eq!(taglia(&"a".repeat(300), 140).chars().count(), 140);
    }
}
