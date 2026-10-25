//! Prove del contratto dell'API (Fase 2.B-bis): riferimenti inesistenti, assistenti archiviati,
//! limiti dei testi e del corpo, impostazioni, riordino delle sezioni.

mod comune;

use comune::*;

/// Chiama e restituisce (stato, codice d'errore).
async fn esito(p: &Prova, c: &str, metodo: &str, uri: &str, corpo: Value) -> (StatusCode, Value) {
    let (s, v, _) = p.chiama(metodo, uri, Some(corpo), Some(c)).await;
    (s, v["errore"]["codice"].clone())
}

#[tokio::test]
async fn riferimenti_inesistenti_con_codici_dedicati() {
    let p = prova(5).await;
    let c = p.entra().await;
    let (aid, sid) = p.sessione(&c).await;
    let a = format!("/api/v1/assistenti/{aid}");
    let s = format!("/api/v1/sessioni/{sid}");
    let casi = [
        (
            "POST",
            "/api/v1/assistenti".to_string(),
            json!({ "nome": "X", "sezione_id": "no" }),
            StatusCode::NOT_FOUND,
            "sezione_non_trovata",
        ),
        (
            "POST",
            "/api/v1/assistenti".to_string(),
            json!({ "nome": "X", "provider_predefinito_id": "no" }),
            StatusCode::NOT_FOUND,
            "provider_non_trovato",
        ),
        (
            "PATCH",
            a.clone(),
            json!({ "sezione_id": "no" }),
            StatusCode::NOT_FOUND,
            "sezione_non_trovata",
        ),
        (
            "PATCH",
            a.clone(),
            json!({ "modello_predefinito_id": "no" }),
            StatusCode::CONFLICT,
            "modello_non_disponibile",
        ),
        (
            "POST",
            format!("{a}/sessioni"),
            json!({ "provider_id": "no" }),
            StatusCode::NOT_FOUND,
            "provider_non_trovato",
        ),
        (
            "POST",
            format!("{a}/sessioni"),
            json!({ "modello_id": "no" }),
            StatusCode::CONFLICT,
            "modello_non_disponibile",
        ),
        (
            "POST",
            format!("{a}/sessioni"),
            json!({ "macchina_id": "no" }),
            StatusCode::BAD_REQUEST,
            "riferimento_non_trovato",
        ),
        (
            "POST",
            "/api/v1/assistenti/no/sessioni".to_string(),
            json!({}),
            StatusCode::NOT_FOUND,
            "assistente_non_trovato",
        ),
        (
            "PATCH",
            s.clone(),
            json!({ "provider_id": "no" }),
            StatusCode::NOT_FOUND,
            "provider_non_trovato",
        ),
        (
            "PATCH",
            "/api/v1/sessioni/no".to_string(),
            json!({ "titolo": "x" }),
            StatusCode::NOT_FOUND,
            "sessione_non_trovata",
        ),
        (
            "POST",
            format!("{s}/invii"),
            json!({ "id": "01RIF0000000000000000001", "testo": "x", "provider": "no" }),
            StatusCode::NOT_FOUND,
            "provider_non_trovato",
        ),
        (
            "POST",
            "/api/v1/sessioni/no/invii".to_string(),
            json!({ "id": "01RIF0000000000000000002", "testo": "x" }),
            StatusCode::NOT_FOUND,
            "sessione_non_trovata",
        ),
    ];
    for (metodo, uri, corpo, stato, codice) in casi {
        let (st, v, _) = p.chiama(metodo, &uri, Some(corpo), Some(&c)).await;
        assert_eq!(
            (st, v["errore"]["codice"].as_str()),
            (stato, Some(codice)),
            "{metodo} {uri}: {v}"
        );
        assert!(!v.to_string().contains("FOREIGN KEY"), "{v}");
    }
    let invii: i64 =
        p.db.con(|k| {
            Ok(k.query_row("SELECT COUNT(*) FROM invio", [], |r| r.get(0))
                .unwrap())
        })
        .unwrap();
    assert_eq!(invii, 0, "nessun invio orfano");
}

#[tokio::test]
async fn assistente_archiviato_rifiuta_invii_e_sessioni() {
    let p = prova(5).await;
    let c = p.entra().await;
    let (aid, sid) = p.sessione(&c).await;
    let (s, _, _) = p
        .chiama(
            "DELETE",
            &format!("/api/v1/assistenti/{aid}"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let invio = json!({ "id": "01ARC0000000000000000001", "testo": "ciao" });
    let r = esito(
        &p,
        &c,
        "POST",
        &format!("/api/v1/sessioni/{sid}/invii"),
        invio,
    )
    .await;
    assert_eq!(r, (StatusCode::CONFLICT, json!("assistente_archiviato")));
    let r = esito(
        &p,
        &c,
        "POST",
        &format!("/api/v1/assistenti/{aid}/sessioni"),
        json!({}),
    )
    .await;
    assert_eq!(r, (StatusCode::CONFLICT, json!("assistente_archiviato")));
    // Le sessioni esistenti restano leggibili.
    let (s, v, _) = p
        .chiama("GET", &format!("/api/v1/sessioni/{sid}"), None, Some(&c))
        .await;
    assert_eq!((s, v["id"].as_str()), (StatusCode::OK, Some(sid.as_str())));
    let (s, _, _) = p
        .chiama(
            "GET",
            &format!("/api/v1/sessioni/{sid}/righe?dopo_aggiornata_seq=0"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn testi_troppo_lunghi_e_corpo_troppo_grande() {
    let p = prova(5).await;
    let c = p.entra().await;
    let (aid, sid) = p.sessione(&c).await;
    let lungo = |n: usize| "à".repeat(n);
    let a = format!("/api/v1/assistenti/{aid}");
    let casi = [
        (
            "POST",
            "/api/v1/assistenti".to_string(),
            json!({ "nome": lungo(81) }),
        ),
        (
            "POST",
            "/api/v1/assistenti".to_string(),
            json!({ "nome": "X", "titolo": lungo(121) }),
        ),
        ("PATCH", a.clone(), json!({ "descrizione": lungo(2001) })),
        (
            "POST",
            format!("{a}/sessioni"),
            json!({ "titolo": lungo(201) }),
        ),
        (
            "PATCH",
            format!("/api/v1/sessioni/{sid}"),
            json!({ "titolo": lungo(201) }),
        ),
        (
            "POST",
            format!("/api/v1/sessioni/{sid}/invii"),
            json!({ "id": "01LUN0000000000000000001", "testo": "a".repeat(100_001) }),
        ),
        (
            "PUT",
            "/api/v1/impostazioni".to_string(),
            json!({ "valori": { lungo(81): 1 } }),
        ),
    ];
    for (metodo, uri, corpo) in casi {
        let r = esito(&p, &c, metodo, &uri, corpo).await;
        assert_eq!(
            r,
            (StatusCode::BAD_REQUEST, json!("testo_troppo_lungo")),
            "{metodo} {uri}"
        );
    }
    // Al limite passa.
    let (s, _, _) = p
        .chiama(
            "POST",
            "/api/v1/assistenti",
            Some(json!({ "nome": lungo(80) })),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let grande = json!({ "nome": "a".repeat(3 * 1024 * 1024) });
    let r = esito(&p, &c, "POST", "/api/v1/assistenti", grande).await;
    assert_eq!(
        r,
        (StatusCode::PAYLOAD_TOO_LARGE, json!("corpo_troppo_grande"))
    );
}

/// (stato del turno, errore_codice, stato dell'invio) per un turno.
fn esito_turno(p: &Prova, turno: &str) -> (String, Option<String>, String) {
    p.db.con(|k| {
        Ok(k.query_row(
            "SELECT t.stato, t.errore_codice, i.stato FROM turno t JOIN invio i ON i.id = t.invio_id
             WHERE t.id = ?1",
            [turno],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap())
    })
    .unwrap()
}

#[tokio::test]
async fn invio_consegnato_a_fine_turno_e_codice_del_provider() {
    let p = prova(300).await;
    let c = p.entra().await;
    let (_, sid) = p.sessione(&c).await;
    let f = p
        .invia(&c, &sid, "01FIN0000000000000000001", "x [fallisci]")
        .await;
    let t = f["turno_id"].as_str().unwrap().to_string();
    assert_eq!(p.aspetta_turno(&c, &t).await, "fallito");
    assert_eq!(
        esito_turno(&p, &t),
        (
            "fallito".into(),
            Some("finto_fallito".into()),
            "consegnato".into()
        )
    );
    let righe = p.righe(&c, &sid, 0).await;
    let avviso = righe["righe"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["genere"] == "avviso")
        .unwrap()
        .clone();
    assert_eq!(avviso["corpo"]["codice"], "finto_fallito");

    let i = p.invia(&c, &sid, "01FIN0000000000000000002", "lungo").await;
    let t = i["turno_id"].as_str().unwrap().to_string();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (s, _, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/turni/{t}/interrompi"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    for _ in 0..100 {
        if esito_turno(&p, &t).2 == "consegnato" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        esito_turno(&p, &t),
        ("interrotto".into(), None, "consegnato".into())
    );
}

#[tokio::test]
async fn impostazioni_col_contratto_valori() {
    let p = prova(5).await;
    let c = p.entra().await;
    let u = "/api/v1/impostazioni";
    let (s, v, _) = p
        .chiama(
            "PUT",
            u,
            Some(json!({ "valori": { "a": 1, "b": { "x": true } } })),
            Some(&c),
        )
        .await;
    assert_eq!(
        (s, v["valori"]["a"].clone(), v["valori"]["b"]["x"].clone()),
        (StatusCode::OK, json!(1), json!(true))
    );
    // Le chiavi non date restano; l'oggetto piatto di prima non è più accettato.
    let (_, v, _) = p
        .chiama("PUT", u, Some(json!({ "valori": { "a": 2 } })), Some(&c))
        .await;
    assert_eq!(
        (v["valori"]["a"].clone(), v["valori"]["b"]["x"].clone()),
        (json!(2), json!(true))
    );
    let r = esito(&p, &c, "PUT", u, json!({ "a": 3 })).await;
    assert_eq!(r, (StatusCode::BAD_REQUEST, json!("richiesta_non_valida")));
    for k in [
        "accesso.totp_ultimo_passo",
        "server.segreto_cookie",
        "accesso.nuova",
    ] {
        let r = esito(&p, &c, "PUT", u, json!({ "valori": { k: 1 } })).await;
        assert_eq!(r, (StatusCode::FORBIDDEN, json!("chiave_riservata")), "{k}");
    }
    let (_, v, _) = p.chiama("GET", u, None, Some(&c)).await;
    let chiavi: Vec<&String> = v["valori"].as_object().unwrap().keys().collect();
    assert_eq!(chiavi, ["a", "b"]);
}

#[tokio::test]
async fn riordino_delle_sezioni() {
    let p = prova(5).await;
    let c = p.entra().await;
    for (id, nome, ordine) in [("lavoro", "Lavoro", 1), ("casa", "Casa", 2)] {
        let corpo = json!({ "id": id, "nome": nome, "ordine": ordine });
        let (s, _, _) = p
            .chiama("POST", "/api/v1/sezioni", Some(corpo), Some(&c))
            .await;
        assert_eq!(s, StatusCode::OK);
    }
    let ordine = |v: &Value| -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap().to_string())
            .filter(|id| id == "lavoro" || id == "casa")
            .collect()
    };
    let (_, v, _) = p.chiama("GET", "/api/v1/sezioni", None, Some(&c)).await;
    assert_eq!(ordine(&v), ["lavoro", "casa"]);
    // Un elenco [{id, ordine?, nome?, chiusa?}] riordina in un colpo.
    let modifiche = json!([
        { "id": "casa", "ordine": 0, "nome": "Casa mia" },
        { "id": "lavoro", "ordine": 5, "chiusa": 1 }
    ]);
    let (s, v, _) = p
        .chiama("PATCH", "/api/v1/sezioni", Some(modifiche), Some(&c))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(ordine(&v), ["casa", "lavoro"]);
    let (_, b, _) = p.chiama("GET", "/api/v1/barra", None, Some(&c)).await;
    assert_eq!(ordine(&b), ["casa", "lavoro"]);
    let casa = b
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "casa")
        .unwrap();
    let lavoro = b
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "lavoro")
        .unwrap();
    assert_eq!(
        (casa["nome"].clone(), lavoro["chiusa"].clone()),
        (json!("Casa mia"), json!(1))
    );
    // Una sezione che non esiste annulla tutto il riordino.
    let r = esito(
        &p,
        &c,
        "PATCH",
        "/api/v1/sezioni",
        json!([{ "id": "casa", "ordine": 9 }, { "id": "no", "ordine": 1 }]),
    )
    .await;
    assert_eq!(r, (StatusCode::NOT_FOUND, json!("sezione_non_trovata")));
    let (_, v, _) = p.chiama("GET", "/api/v1/sezioni", None, Some(&c)).await;
    assert_eq!(ordine(&v), ["casa", "lavoro"], "niente a metà");
    let r = esito(&p, &c, "PATCH", "/api/v1/sezioni", json!({ "id": "casa" })).await;
    assert_eq!(r, (StatusCode::BAD_REQUEST, json!("richiesta_non_valida")));
}
