//! Prove dell'accesso: password, TOTP, cookie, uscita e limite dei tentativi falliti.

mod comune;

use comune::*;

#[tokio::test]
async fn accesso_con_password_e_totp() {
    let p = prova(5).await;
    let (s, v, _) = p.chiama("GET", "/api/v1/salute", None, None).await;
    assert_eq!(
        (s, v["ok"].clone(), v["db"]["schema"].clone()),
        (
            StatusCode::OK,
            json!(true),
            json!(xbot_db::VERSIONE_SCHEMA.to_string())
        )
    );

    let (s, v, _) = p.chiama("GET", "/api/v1/io", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(v["errore"]["codice"], "non_autorizzato");

    let sbagliata = json!({ "password": "non-e-questa", "codice_totp": p.codice() });
    let (s, v, c) = p
        .chiama("POST", "/api/v1/accesso", Some(sbagliata), None)
        .await;
    assert_eq!(
        (s, v["errore"]["codice"].clone()),
        (StatusCode::UNAUTHORIZED, json!("accesso_negato"))
    );
    assert!(c.is_none());

    let codice_sbagliato = if p.codice() == "000000" {
        "111111"
    } else {
        "000000"
    };
    let senza_totp = json!({ "password": PASSWORD, "codice_totp": codice_sbagliato });
    let (s, _, _) = p
        .chiama("POST", "/api/v1/accesso", Some(senza_totp), None)
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = p
        .chiama(
            "POST",
            "/api/v1/accesso",
            Some(json!({ "password": PASSWORD })),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let c = p.entra().await;
    assert!(c.starts_with("xbot_accesso="));
    let (s, io, _) = p.chiama("GET", "/api/v1/io", None, Some(&c)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(io["nome"], "Mario");
    let provider = io["provider"].as_array().unwrap();
    assert_eq!(provider[0]["id"], "finto");
    assert!(
        provider.iter().all(|p| p.get("programma").is_none()),
        "il percorso del programma non esce da /io: {io}"
    );
    assert_eq!(io["accesso"]["dispositivo"], "prova");

    // Un cookie manomesso non passa.
    let (s, _, _) = p
        .chiama("GET", "/api/v1/io", None, Some("xbot_accesso=falso"))
        .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let (s, _, _) = p.chiama("DELETE", "/api/v1/accesso", None, Some(&c)).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = p.chiama("GET", "/api/v1/io", None, Some(&c)).await;
    assert_eq!(
        s,
        StatusCode::UNAUTHORIZED,
        "dopo l'uscita il cookie non vale più"
    );
}

#[tokio::test]
async fn accesso_bloccato_dopo_cinque_errori() {
    let p = prova(5).await;
    let sbagliata = json!({ "password": "non-e-questa", "codice_totp": "000000" });
    for _ in 0..5 {
        let (s, _, _) = p
            .chiama("POST", "/api/v1/accesso", Some(sbagliata.clone()), None)
            .await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    let giusta = json!({ "password": PASSWORD, "codice_totp": p.codice() });
    let (s, v, c) = p
        .chiama("POST", "/api/v1/accesso", Some(giusta), None)
        .await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(v["errore"]["codice"], "troppi_tentativi");
    assert!(c.is_none());
}

#[tokio::test]
async fn codice_totp_vale_una_volta() {
    let p = prova(5).await;
    let totp = totp_rs::Totp::from_url(&p.url_totp).unwrap();
    let ora = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let accedi = |codice: String| {
        let corpo = json!({ "password": PASSWORD, "codice_totp": codice });
        let p = &p;
        async move {
            p.chiama("POST", "/api/v1/accesso", Some(corpo), None)
                .await
                .0
        }
    };
    let adesso = totp.generate(ora).to_string();
    assert_eq!(accedi(adesso.clone()).await, StatusCode::OK);
    assert_eq!(
        accedi(adesso).await,
        StatusCode::UNAUTHORIZED,
        "riuso rifiutato"
    );
    // Il passo dopo (tollerato dallo skew) vale; poi quello di prima no.
    assert_eq!(
        accedi(totp.generate(ora + 30).to_string()).await,
        StatusCode::OK
    );
    assert_eq!(
        accedi(totp.generate(ora).to_string()).await,
        StatusCode::UNAUTHORIZED
    );
}

/// Un tentativo sbagliato da `127.0.0.1` con `X-Forwarded-For`.
async fn sbaglia_da(p: &Prova, xff: &str) -> StatusCode {
    let corpo = json!({ "password": "non-e-questa", "codice_totp": "000000" });
    let mut req = Request::builder()
        .method("POST")
        .uri("/api/v1/accesso")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-forwarded-for", xff)
        .body(Body::from(corpo.to_string()))
        .unwrap();
    let conn: std::net::SocketAddr = "127.0.0.1:50000".parse().unwrap();
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(conn));
    p.app.clone().oneshot(req).await.unwrap().status()
}

#[tokio::test]
async fn limite_per_cliente_dietro_proxy() {
    let p = prova_con_proxy(vec![Arc::new(Finto::nuovo())], "127.0.0.1").await;
    for _ in 0..5 {
        assert_eq!(
            sbaglia_da(&p, "203.0.113.9").await,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        sbaglia_da(&p, "203.0.113.9").await,
        StatusCode::TOO_MANY_REQUESTS
    );
    // Un altro client dietro lo stesso proxy non è bloccato.
    assert_eq!(
        sbaglia_da(&p, "198.51.100.7").await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn senza_proxy_fidato_l_intestazione_si_ignora() {
    let p = prova(5).await;
    for _ in 0..5 {
        assert_eq!(
            sbaglia_da(&p, "203.0.113.9").await,
            StatusCode::UNAUTHORIZED
        );
    }
    // Cambiare X-Forwarded-For non aggira il limite: conta la connessione (127.0.0.1).
    assert_eq!(
        sbaglia_da(&p, "198.51.100.7").await,
        StatusCode::TOO_MANY_REQUESTS
    );
}
