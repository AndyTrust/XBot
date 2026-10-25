//! Prove S1: la cassaforte (sezione 1 del contratto 2.D).

mod aiuti_s1;
mod comune;

use aiuti_s1::*;
use comune::*;

#[tokio::test]
async fn cassaforte_chiusa_risponde_503() {
    let p = prova_s1(None).await;
    let c = p.entra().await;
    for (m, uri) in [
        ("GET", "/api/v1/cassaforte"),
        ("POST", "/api/v1/cassaforte/x/rivela"),
        ("GET", "/api/v1/cassaforte/x/usi"),
    ] {
        let (s, v, _) = p.chiama(m, uri, None, Some(&c)).await;
        assert_eq!(
            (s, v["errore"]["codice"].as_str()),
            (StatusCode::SERVICE_UNAVAILABLE, Some("cassaforte_chiusa")),
            "{uri}"
        );
    }
}

#[tokio::test]
async fn giro_accesso_carta_contatto_segreto() {
    let p = prova_s1(Some(FRASE)).await;
    let c = p.entra().await;
    let (s, v, _) = p.chiama("GET", "/api/v1/cassaforte", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "{v}");

    let corpo = json!({"genere": "accesso", "titolo": "Posta", "siti": ["https://posta.example"],
        "riferimento": {"utente": "mario"}, "campi": {"password": "p4ss", "passphrase": "frase lunga"}});
    let (s, v, _) = p
        .chiama("POST", "/api/v1/cassaforte", Some(corpo), Some(&c))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let id = v["id"].as_str().unwrap().to_string();

    // Elenco: mai i campi cifrati.
    let (s, v, _) = p
        .chiama(
            "GET",
            "/api/v1/cassaforte?genere=accesso&cerca=posta",
            None,
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["voci"].as_array().unwrap().len(), 1);
    assert!(!v.to_string().contains("p4ss"));
    assert_eq!(v["voci"][0]["riferimento"]["utente"], "mario");

    // PATCH: solo le chiavi date.
    let (s, _, _) = p
        .chiama(
            "PATCH",
            &format!("/api/v1/cassaforte/{id}"),
            Some(json!({"campi": {"password": "nuova"}})),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, v, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/cassaforte/{id}/rivela"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        v["campi"],
        json!({"password": "nuova", "passphrase": "frase lunga"})
    );
    let (_, v, _) = p
        .chiama(
            "GET",
            &format!("/api/v1/cassaforte/{id}/usi"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(v["usi"][0]["azione"], "riferimento");

    // Carta: Luhn e brand/last4 dal server.
    let carta = json!({"genere": "carta", "titolo": "Visa", "riferimento": {"exp_month": 7, "exp_year": 2029},
        "campi": {"numero": "4111 1111 1111 1111", "cvc": "123"}});
    let (s, v, _) = p
        .chiama("POST", "/api/v1/cassaforte", Some(carta), Some(&c))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, v, _) = p
        .chiama(
            "GET",
            &format!("/api/v1/cassaforte/{}", v["id"].as_str().unwrap()),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(
        (
            v["riferimento"]["brand"].as_str(),
            v["riferimento"]["last4"].as_str()
        ),
        (Some("visa"), Some("1111"))
    );
    let sbagliata =
        json!({"genere": "carta", "titolo": "X", "campi": {"numero": "4111111111111112"}});
    let (s, v, _) = p
        .chiama("POST", "/api/v1/cassaforte", Some(sbagliata), Some(&c))
        .await;
    assert_eq!(
        (s, v["errore"]["codice"].as_str()),
        (StatusCode::BAD_REQUEST, Some("carta_non_valida"))
    );

    // Contatto senza campi; segreto con assistente.
    let (s, _, _) = p
        .chiama(
            "POST",
            "/api/v1/cassaforte",
            Some(json!({"genere": "contatto", "titolo": "Anna",
            "riferimento": {"nome": "Anna", "email": "anna@example.com"}})),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (aid, _) = p.sessione(&c).await;
    let (s, v, _) = p
        .chiama("POST", "/api/v1/cassaforte", Some(json!({"genere": "segreto", "titolo": "GitHub",
            "riferimento": {"variabile": "GH_TOKEN"}, "campi": {"valore": "ghp_123"}, "assistenti": [aid]})), Some(&c))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let k = p.stato.servizi.chiave().unwrap();
    let a2 = aid.clone();
    let segreti =
        p.db.con(move |conn| xbot_db::cassaforte_voci::segreti_per_assistente(conn, &k, &a2, None))
            .unwrap();
    assert_eq!(segreti[0].0, "GH_TOKEN");
    assert_eq!(segreti[0].1.esponi(), "ghp_123");

    // Errori: genere sconosciuto, voce mancante, DELETE.
    let (s, _, _) = p
        .chiama("GET", "/api/v1/cassaforte?genere=boh", None, Some(&c))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v, _) = p
        .chiama("POST", "/api/v1/cassaforte/nessuna/rivela", None, Some(&c))
        .await;
    assert_eq!(
        (s, v["errore"]["codice"].as_str()),
        (StatusCode::NOT_FOUND, Some("voce_non_trovata"))
    );
    let (s, _, _) = p
        .chiama(
            "DELETE",
            &format!("/api/v1/cassaforte/{id}"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _, _) = p
        .chiama(
            "DELETE",
            &format!("/api/v1/cassaforte/{id}"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn importa_csv_bitwarden_e_formato_sconosciuto() {
    let p = prova_s1(Some(FRASE)).await;
    let c = p.entra().await;
    let csv = "folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp\n\
        ,,login,Posta,,,0,https://posta.example,mario,p1,\n\
        ,,login,Banca,nota,,0,https://banca.example,anna,p2,\n";
    let (s, v) = multipart(
        &p,
        "/api/v1/cassaforte/importa",
        &c,
        "file",
        "bw.csv",
        "text/csv",
        csv.as_bytes(),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        (
            v["importati"].as_i64(),
            v["saltati"].as_i64(),
            v["formato"].as_str()
        ),
        (Some(2), Some(0), Some("bitwarden"))
    );
    let (_, v, _) = p.chiama("GET", "/api/v1/cassaforte", None, Some(&c)).await;
    assert_eq!(v["voci"][0]["origine"], "csv:bitwarden");
    let (s, v) = multipart(
        &p,
        "/api/v1/cassaforte/importa",
        &c,
        "file",
        "x.csv",
        "text/csv",
        b"a,b\n1,2\n",
    )
    .await;
    assert_eq!(
        (s, v["errore"]["codice"].as_str()),
        (StatusCode::BAD_REQUEST, Some("richiesta_non_valida"))
    );
}
