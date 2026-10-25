//! Aiuti comuni alle prove di integrazione: server in memoria, accesso, chiamate JSON.
#![allow(dead_code, unused_imports)]

pub use std::path::Path;
pub use std::sync::Arc;
pub use std::time::Duration;

pub use axum::Router;
pub use axum::body::Body;
pub use axum::http::{Request, StatusCode, header};
pub use base64::Engine;
pub use http_body_util::BodyExt;
pub use serde_json::{Value, json};
pub use tower::ServiceExt;
pub use xbot_core::SegretoTesto;
pub use xbot_db::Db;
pub use xbot_provider::{Finto, Provider};
pub use xbot_server::{Stato, accesso, app};

pub const PASSWORD: &str = "password-di-prova-lunga";

pub struct Prova {
    pub stato: Stato,
    pub app: Router,
    pub db: Db,
    pub url_totp: String,
}

pub async fn prova(ritardo_ms: u64) -> Prova {
    prova_con(vec![Arc::new(Finto::con_ritardo(Duration::from_millis(
        ritardo_ms,
    )))])
    .await
}

pub async fn prova_con(provider: Vec<Arc<dyn Provider>>) -> Prova {
    prova_con_proxy(provider, "").await
}

/// Come `prova_con`, con `XBOT_PROXY_FIDATO` = `proxy`.
pub async fn prova_con_proxy(provider: Vec<Arc<dyn Provider>>, proxy: &str) -> Prova {
    let db = Db::apri(":memory:").unwrap();
    let segreto = base64::engine::general_purpose::STANDARD.encode([9u8; 32]);
    let stato = Stato::prepara_con(
        db.clone(),
        provider,
        Some(SegretoTesto::nuovo(segreto)),
        false,
        xbot_server::cliente::Rete::leggi_lista(proxy).unwrap(),
    )
    .await
    .unwrap();
    let url_totp = accesso::impianta(
        &db,
        &stato.chiave_totp,
        "Mario",
        &SegretoTesto::nuovo(PASSWORD),
        true,
    )
    .unwrap()
    .unwrap();
    Prova {
        app: app(stato.clone(), Path::new("web-che-non-esiste")),
        stato,
        db,
        url_totp,
    }
}

impl Prova {
    pub fn codice(&self) -> String {
        totp_rs::Totp::from_url(&self.url_totp)
            .unwrap()
            .generate_current()
            .to_string()
    }

    pub async fn chiama(
        &self,
        metodo: &str,
        uri: &str,
        corpo: Option<Value>,
        cookie: Option<&str>,
    ) -> (StatusCode, Value, Option<String>) {
        let mut r = Request::builder().method(metodo).uri(uri);
        if let Some(c) = cookie {
            r = r.header(header::COOKIE, c);
        }
        let req = match corpo {
            Some(v) => r
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(v.to_string()))
                .unwrap(),
            None => r.body(Body::empty()).unwrap(),
        };
        let risp = self.app.clone().oneshot(req).await.unwrap();
        let stato = risp.status();
        let cookie = risp
            .headers()
            .get(header::SET_COOKIE)
            .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
        let byte = risp.into_body().collect().await.unwrap().to_bytes();
        let v = serde_json::from_slice(&byte).unwrap_or(Value::Null);
        (stato, v, cookie)
    }

    pub async fn entra(&self) -> String {
        let corpo =
            json!({ "password": PASSWORD, "codice_totp": self.codice(), "dispositivo": "prova" });
        let (s, v, c) = self
            .chiama("POST", "/api/v1/accesso", Some(corpo), None)
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        c.unwrap()
    }

    /// Crea assistente e sessione, restituisce l'id della sessione.
    pub async fn sessione(&self, c: &str) -> (String, String) {
        let (s, a, _) = self
            .chiama(
                "POST",
                "/api/v1/assistenti",
                Some(json!({ "nome": "Atena" })),
                Some(c),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{a}");
        let aid = a["id"].as_str().unwrap().to_string();
        let uri = format!("/api/v1/assistenti/{aid}/sessioni");
        let (s, sess, _) = self
            .chiama("POST", &uri, Some(json!({ "titolo": "prova" })), Some(c))
            .await;
        assert_eq!(s, StatusCode::OK, "{sess}");
        (aid, sess["id"].as_str().unwrap().to_string())
    }

    pub async fn invia(&self, c: &str, sid: &str, id: &str, testo: &str) -> Value {
        let uri = format!("/api/v1/sessioni/{sid}/invii");
        let (s, v, _) = self
            .chiama(
                "POST",
                &uri,
                Some(json!({ "id": id, "testo": testo })),
                Some(c),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v
    }

    pub async fn righe(&self, c: &str, sid: &str, dopo: i64) -> Value {
        let uri = format!("/api/v1/sessioni/{sid}/righe?dopo_aggiornata_seq={dopo}");
        let (s, v, _) = self.chiama("GET", &uri, None, Some(c)).await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v
    }

    /// Aspetta che il turno non sia più in corso; restituisce lo stato finale.
    pub async fn aspetta_turno(&self, c: &str, turno: &str) -> String {
        for _ in 0..200 {
            let uri = format!("/api/v1/turni/{turno}/interrompi");
            let n: i64 = self
                .db
                .con(|conn| {
                    Ok(conn
                        .query_row(
                            "SELECT COUNT(*) FROM turno WHERE id = ?1 AND stato = 'in_corso'",
                            [turno],
                            |r| r.get(0),
                        )
                        .unwrap())
                })
                .unwrap();
            if n == 0 {
                // Su un turno chiuso «interrompi» non cambia nulla e dice lo stato.
                let (_, t, _) = self.chiama("POST", &uri, None, Some(c)).await;
                return t["stato"].as_str().unwrap().to_string();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("il turno non è finito");
    }
}
