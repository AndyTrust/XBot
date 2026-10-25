//! Prove del dispositivo fidato: cookie `xbot_fidato`, password sola, elenco e revoca.

mod comune;

use comune::*;

/// Una risposta con tutti i `Set-Cookie` (nome=valore, e se è una cancellazione).
struct Risp {
    stato: StatusCode,
    corpo: Value,
    cookie: Vec<String>,
}

impl Risp {
    /// Il valore `nome=…` del cookie messo (non cancellato) con questo nome.
    fn messo(&self, nome: &str) -> Option<String> {
        self.cookie
            .iter()
            .find(|c| c.starts_with(&format!("{nome}=")) && !c.contains("Max-Age=0"))
            .map(|c| c.split(';').next().unwrap().to_string())
    }
    fn cancellato(&self, nome: &str) -> bool {
        self.cookie
            .iter()
            .any(|c| c.starts_with(&format!("{nome}=")) && c.contains("Max-Age=0"))
    }
}

async fn chiama(p: &Prova, metodo: &str, uri: &str, corpo: Option<Value>, cookie: &str) -> Risp {
    let mut r = Request::builder().method(metodo).uri(uri);
    if !cookie.is_empty() {
        r = r.header(header::COOKIE, cookie);
    }
    let req = match corpo {
        Some(v) => r
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => r.body(Body::empty()).unwrap(),
    };
    let risp = p.app.clone().oneshot(req).await.unwrap();
    let stato = risp.status();
    let cookie = risp
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect();
    let byte = risp.into_body().collect().await.unwrap().to_bytes();
    Risp {
        stato,
        corpo: serde_json::from_slice(&byte).unwrap_or(Value::Null),
        cookie,
    }
}

/// Accesso completo con `fidato: true`: (cookie di sessione, cookie fidato, id).
async fn entra_fidato(p: &Prova, nome: &str) -> (String, String, String) {
    let corpo = json!({
        "password": PASSWORD, "codice_totp": p.codice(), "dispositivo": nome, "fidato": true
    });
    let r = chiama(p, "POST", "/api/v1/accesso", Some(corpo), "").await;
    assert_eq!(r.stato, StatusCode::OK, "{}", r.corpo);
    assert_eq!(r.corpo["fidato"], true);
    let id = r.corpo["dispositivo_fidato_id"]
        .as_str()
        .unwrap()
        .to_string();
    let f = r.messo("xbot_fidato").expect("cookie fidato");
    let pieno = r
        .cookie
        .iter()
        .find(|c| c.starts_with("xbot_fidato="))
        .unwrap();
    for parte in ["HttpOnly", "SameSite=Lax", "Path=/", "Max-Age=34560000"] {
        assert!(pieno.contains(parte), "{pieno}");
    }
    (r.messo("xbot_accesso").unwrap(), f, id)
}

async fn password_sola(p: &Prova, cookie: &str) -> Risp {
    let corpo = json!({ "password": PASSWORD, "dispositivo": "telefono" });
    chiama(p, "POST", "/api/v1/accesso", Some(corpo), cookie).await
}

/// Il codice TOTP vale una volta per passo: fra due accessi completi nello stesso passo si
/// dimentica l'ultimo passo usato (al posto di aspettare 30 secondi).
fn dimentica_passo(p: &Prova) {
    p.db.con(|c| {
        c.execute(
            "DELETE FROM impostazione WHERE chiave = 'accesso.totp_ultimo_passo'",
            [],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn fidato_basta_la_password() {
    let p = prova(5).await;
    // Senza cookie e senza codice: totp_richiesto (la password è giusta).
    let r = password_sola(&p, "").await;
    assert_eq!(
        (r.stato, r.corpo["errore"]["codice"].clone()),
        (StatusCode::UNAUTHORIZED, json!("totp_richiesto"))
    );
    assert!(r.messo("xbot_accesso").is_none());

    let (sessione, fidato, id) = entra_fidato(&p, "Mac").await;
    let (s, _, _) = p.chiama("GET", "/api/v1/io", None, Some(&sessione)).await;
    assert_eq!(s, StatusCode::OK);

    // Con il cookie fidato basta la password.
    let prima: i64 =
        p.db.con(|c| {
            Ok(c.query_row(
                "SELECT ultimo_uso_ms FROM dispositivo_fidato WHERE id = ?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap())
        })
        .unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    let r = password_sola(&p, &fidato).await;
    assert_eq!(r.stato, StatusCode::OK, "{}", r.corpo);
    assert_eq!(
        (
            r.corpo["fidato"].clone(),
            r.corpo["dispositivo_fidato_id"].clone()
        ),
        (json!(true), json!(id))
    );
    // il cookie fidato si rinnova a ogni uso (stesso valore, scadenza spostata in avanti)
    assert!(r.messo("xbot_fidato").is_some(), "cookie fidato rinnovato");
    let nuova = r.messo("xbot_accesso").unwrap();
    let (s, _, _) = p.chiama("GET", "/api/v1/io", None, Some(&nuova)).await;
    assert_eq!(s, StatusCode::OK);
    let dopo: i64 =
        p.db.con(|c| {
            Ok(c.query_row(
                "SELECT ultimo_uso_ms FROM dispositivo_fidato WHERE id = ?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap())
        })
        .unwrap();
    assert!(dopo > prima, "ultimo_uso_ms aggiornato");

    // Password sbagliata con il cookie: accesso_negato, e il cookie resta valido.
    let corpo = json!({ "password": "non-e-questa" });
    let r = chiama(&p, "POST", "/api/v1/accesso", Some(corpo), &fidato).await;
    assert_eq!(
        (r.stato, r.corpo["errore"]["codice"].clone()),
        (StatusCode::UNAUTHORIZED, json!("accesso_negato"))
    );
    assert!(!r.cancellato("xbot_fidato"));

    // Cookie manomesso: ignorato, quindi serve di nuovo il TOTP.
    let valore = fidato.split_once('=').unwrap().1;
    let mut byte = valore.as_bytes().to_vec();
    let ultimo = byte.len() - 1;
    byte[ultimo] = if byte[ultimo] == b'A' { b'B' } else { b'A' };
    let manomesso = format!("xbot_fidato={}", String::from_utf8(byte).unwrap());
    let r = password_sola(&p, &manomesso).await;
    assert_eq!(r.corpo["errore"]["codice"], "totp_richiesto");
    let r = password_sola(&p, "xbot_fidato=falso").await;
    assert_eq!(r.corpo["errore"]["codice"], "totp_richiesto");

    // L'uscita non tocca il dispositivo fidato.
    let r = chiama(&p, "DELETE", "/api/v1/accesso", None, &nuova).await;
    assert_eq!(r.stato, StatusCode::OK);
    assert!(!r.cancellato("xbot_fidato"));
    assert_eq!(password_sola(&p, &fidato).await.stato, StatusCode::OK);

    // Senza `fidato: true` nessun cookie fidato.
    dimentica_passo(&p);
    let corpo = json!({ "password": PASSWORD, "codice_totp": p.codice() });
    let r = chiama(&p, "POST", "/api/v1/accesso", Some(corpo), "").await;
    assert_eq!(r.stato, StatusCode::OK);
    assert_eq!(r.corpo["fidato"], false);
    assert!(r.corpo["dispositivo_fidato_id"].is_null());
    assert!(r.messo("xbot_fidato").is_none());
}

#[tokio::test]
async fn elenco_e_revoca() {
    let p = prova(5).await;
    let (sessione, fidato_a, id_a) = entra_fidato(&p, "Mac").await;
    dimentica_passo(&p);
    let (_, fidato_b, id_b) = entra_fidato(&p, "S24").await;

    let cookie = format!("{sessione}; {fidato_a}");
    let r = chiama(&p, "GET", "/api/v1/dispositivi-fidati", None, &cookie).await;
    assert_eq!(r.stato, StatusCode::OK, "{}", r.corpo);
    let d = r.corpo["dispositivi"].as_array().unwrap();
    assert_eq!(d.len(), 2);
    for x in d {
        let atteso = x["id"] == json!(id_a);
        assert_eq!(x["corrente"], json!(atteso), "{x}");
        for k in ["nome", "creato_ms", "ultimo_uso_ms", "scade_ms"] {
            assert!(!x[k].is_null(), "{k} in {x}");
        }
        assert!(x.get("hash").is_none());
    }
    // Senza accesso l'elenco non si vede.
    let r = chiama(&p, "GET", "/api/v1/dispositivi-fidati", None, &fidato_a).await;
    assert_eq!(r.stato, StatusCode::UNAUTHORIZED);

    // Revoca del corrente: 204, cookie cancellato, e non vale più.
    let uri = format!("/api/v1/dispositivi-fidati/{id_a}");
    let r = chiama(&p, "DELETE", &uri, None, &cookie).await;
    assert_eq!(r.stato, StatusCode::NO_CONTENT);
    assert!(r.cancellato("xbot_fidato"));
    let r = chiama(&p, "DELETE", &uri, None, &cookie).await;
    assert_eq!(r.stato, StatusCode::NOT_FOUND);
    let r = password_sola(&p, &fidato_a).await;
    assert_eq!(r.corpo["errore"]["codice"], "totp_richiesto");
    assert!(r.cancellato("xbot_fidato"), "cookie revocato cancellato");
    assert_eq!(password_sola(&p, &fidato_b).await.stato, StatusCode::OK);

    // Revoca di un altro: il cookie della richiesta resta.
    dimentica_passo(&p);
    let (_, _fidato_c, id_c) = entra_fidato(&p, "iPad").await;
    let cookie_b = format!("{sessione}; {fidato_b}");
    let uri = format!("/api/v1/dispositivi-fidati/{id_c}");
    let r = chiama(&p, "DELETE", &uri, None, &cookie_b).await;
    assert_eq!(r.stato, StatusCode::NO_CONTENT);
    assert!(!r.cancellato("xbot_fidato"));

    // Revoca di tutti.
    let r = chiama(&p, "DELETE", "/api/v1/dispositivi-fidati", None, &cookie_b).await;
    assert_eq!(
        (r.stato, r.corpo["revocati"].clone()),
        (StatusCode::OK, json!(1))
    );
    assert!(r.cancellato("xbot_fidato"));
    assert_eq!(
        password_sola(&p, &fidato_b).await.corpo["errore"]["codice"],
        "totp_richiesto"
    );
    let r = chiama(&p, "GET", "/api/v1/dispositivi-fidati", None, &sessione).await;
    assert_eq!(r.corpo["dispositivi"], json!([]));
    let _ = id_b;
}

#[tokio::test]
async fn fidato_scaduto_si_cancella() {
    let p = prova(5).await;
    let (_, fidato, id) = entra_fidato(&p, "Mac").await;
    p.db.con(|c| {
        c.execute(
            "UPDATE dispositivo_fidato SET scade_ms = 1 WHERE id = ?1",
            [&id],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
    let r = password_sola(&p, &fidato).await;
    assert_eq!(r.corpo["errore"]["codice"], "totp_richiesto");
    assert!(r.cancellato("xbot_fidato"));
}
