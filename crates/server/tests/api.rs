//! Prove di integrazione dell'API v1 con il provider finto (sezione 3 di ARCHITETTURA).

mod comune;

use comune::*;

#[tokio::test]
async fn giro_completo_righe_riscritte_duplicato_e_azzera() {
    let p = prova(80).await;
    let c = p.entra().await;
    let (aid, sid) = p.sessione(&c).await;

    let esito = p
        .invia(&c, &sid, "01INVIOPROVA000000000000001", "ciao Mario")
        .await;
    assert_eq!(esito["stato"], "accettato");
    assert_eq!(esito["consegna"], "avviato_continuazione");
    let turno = esito["turno_id"].as_str().unwrap().to_string();
    let eco = esito["riga_eco_id"].as_str().unwrap().to_string();

    // Lettura a cursore mentre il testo arriva: la stessa riga torna con aggiornata_seq crescente.
    let mut cursore = 0;
    let mut viste: Vec<(String, i64, i64, String)> = Vec::new();
    for _ in 0..100 {
        let pagina = p.righe(&c, &sid, cursore).await;
        for r in pagina["righe"].as_array().unwrap() {
            cursore = r["aggiornata_seq"].as_i64().unwrap();
            viste.push((
                r["id"].as_str().unwrap().into(),
                r["seq"].as_i64().unwrap(),
                cursore,
                r["genere"].as_str().unwrap().into(),
            ));
        }
        if viste.iter().any(|v| v.3 == "chiamata_strumento") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    assert_eq!(viste[0].0, eco, "prima la riga d'eco dell'utente");
    let risposta = viste
        .iter()
        .find(|v| v.3 == "messaggio" && v.0 != eco)
        .unwrap()
        .0
        .clone();
    let versioni: Vec<&(String, i64, i64, String)> =
        viste.iter().filter(|v| v.0 == risposta).collect();
    assert!(
        versioni.len() >= 2,
        "la riga riscritta va vista più volte: {viste:?}"
    );
    assert!(
        versioni
            .windows(2)
            .all(|w| w[0].1 == w[1].1 && w[0].2 < w[1].2)
    );

    assert_eq!(p.aspetta_turno(&c, &turno).await, "riuscito");
    let tutte = p.righe(&c, &sid, 0).await;
    let testo = tutte["righe"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == risposta.as_str())
        .unwrap()["corpo"]["testo"]
        .clone();
    assert_eq!(testo, "oiraM oaic");
    let quante = tutte["righe"].as_array().unwrap().len();

    // Stesso id: duplicato, niente di nuovo.
    let dup = p
        .invia(&c, &sid, "01INVIOPROVA000000000000001", "ciao Mario")
        .await;
    assert_eq!(dup["stato"], "duplicato");
    assert_eq!(dup["riga_eco_id"], eco.as_str());
    assert_eq!(dup["turno_id"], turno.as_str());
    assert_eq!(
        p.righe(&c, &sid, 0).await["righe"]
            .as_array()
            .unwrap()
            .len(),
        quante
    );

    // La barra vede i non letti.
    let (_, barra, _) = p.chiama("GET", "/api/v1/barra", None, Some(&c)).await;
    let voce = &barra
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "__non_assegnati__")
        .unwrap()["assistenti"][0];
    assert_eq!(voce["assistente_id"], aid.as_str());
    assert!(voce["non_letti"].as_i64().unwrap() >= 1);

    // Azzera: nuova generazione e il cursore vecchio è rifiutato.
    let (s, az, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/sessioni/{sid}/azzera"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!((s, az["generazione"].clone()), (StatusCode::OK, json!(2)));
    let uri = format!("/api/v1/sessioni/{sid}/righe?generazione=1&dopo_aggiornata_seq={cursore}");
    let (s, v, _) = p.chiama("GET", &uri, None, Some(&c)).await;
    assert_eq!(
        (s, v["errore"]["codice"].clone()),
        (StatusCode::CONFLICT, json!("cursore_vecchio"))
    );
    let uri = format!("/api/v1/sessioni/{sid}/righe?generazione=2");
    let (s, v, _) = p.chiama("GET", &uri, None, Some(&c)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(v["righe"].as_array().unwrap().is_empty());
}

/// Legge dal corpo SSE finché trova `cerca` o scade il tempo.
async fn leggi_fino(corpo: &mut Body, accumulato: &mut String, cerca: &str) -> bool {
    let fine = tokio::time::Instant::now() + Duration::from_secs(5);
    while !accumulato.contains(cerca) {
        let resto = fine.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(resto, corpo.frame()).await {
            Ok(Some(Ok(f))) => {
                if let Ok(d) = f.into_data() {
                    accumulato.push_str(&String::from_utf8_lossy(&d));
                }
            }
            _ => return false,
        }
    }
    true
}

#[tokio::test]
async fn sse_consegna_connected_e_rows() {
    let p = prova(5).await;
    let c = p.entra().await;
    let (_, sid) = p.sessione(&c).await;
    let req = Request::builder()
        .uri(format!("/api/v1/sessioni/{sid}/flusso"))
        .header(header::COOKIE, &c)
        .body(Body::empty())
        .unwrap();
    let risp = p.app.clone().oneshot(req).await.unwrap();
    assert_eq!(risp.status(), StatusCode::OK);
    assert_eq!(risp.headers()[header::CONTENT_TYPE], "text/event-stream");
    let mut corpo = risp.into_body();
    let mut testo = String::new();
    assert!(
        leggi_fino(&mut corpo, &mut testo, "event: connected").await,
        "{testo}"
    );
    let esito = p
        .invia(&c, &sid, "01INVIOSSE00000000000000001", "flusso")
        .await;
    assert_eq!(esito["stato"], "accettato");
    assert!(
        leggi_fino(&mut corpo, &mut testo, "event: rows").await,
        "{testo}"
    );
    assert!(
        leggi_fino(&mut corpo, &mut testo, "ossulf").await,
        "il testo rovesciato arriva: {testo}"
    );
    assert!(
        leggi_fino(
            &mut corpo,
            &mut testo,
            "event: turno\ndata: {\"stato\":\"riuscito\""
        )
        .await,
        "{testo}"
    );

    // Azzera durante il flusso: frame cleared.
    let (s, _, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/sessioni/{sid}/azzera"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        leggi_fino(&mut corpo, &mut testo, "event: cleared").await,
        "{testo}"
    );
}

#[tokio::test]
async fn sse_con_generazione_vecchia_dice_cursor_too_old() {
    let p = prova(5).await;
    let c = p.entra().await;
    let (_, sid) = p.sessione(&c).await;
    p.chiama(
        "POST",
        &format!("/api/v1/sessioni/{sid}/azzera"),
        None,
        Some(&c),
    )
    .await;
    let req = Request::builder()
        .uri(format!("/api/v1/sessioni/{sid}/flusso?generazione=1"))
        .header(header::COOKIE, &c)
        .body(Body::empty())
        .unwrap();
    let mut corpo = p.app.clone().oneshot(req).await.unwrap().into_body();
    let mut testo = String::new();
    assert!(
        leggi_fino(&mut corpo, &mut testo, "event: cursor_too_old").await,
        "{testo}"
    );
}

#[tokio::test]
async fn secondo_invio_in_coda_poi_parte() {
    let p = prova(30).await;
    let c = p.entra().await;
    let (_, sid) = p.sessione(&c).await;
    let a = p.invia(&c, &sid, "01CODA0000000000000000000A", "uno").await;
    let b = p.invia(&c, &sid, "01CODA0000000000000000000B", "due").await;
    assert_eq!(a["consegna"], "avviato_continuazione");
    assert_eq!(b["consegna"], "in_coda_prossimo_turno");
    assert!(b["turno_id"].is_null());
    for _ in 0..200 {
        let v = p.righe(&c, &sid, 0).await;
        let testi: Vec<String> = v["righe"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r["corpo"]["testo"].as_str().map(str::to_string))
            .collect();
        if testi.contains(&"eud".to_string()) {
            assert!(testi.contains(&"onu".to_string()));
            let riusciti: i64 =
                p.db.con(|k| {
                    Ok(k.query_row(
                        "SELECT COUNT(*) FROM turno WHERE stato='riuscito'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap())
                })
                .unwrap();
            if riusciti == 2 {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("il secondo invio non è partito");
}

#[tokio::test]
async fn fallito_interrotto_e_rifiutato() {
    let p = prova(5).await;
    let c = p.entra().await;
    let (_, sid) = p.sessione(&c).await;

    let f = p
        .invia(&c, &sid, "01FALLISCI0000000000000001", "prova [fallisci]")
        .await;
    assert_eq!(
        p.aspetta_turno(&c, f["turno_id"].as_str().unwrap()).await,
        "fallito"
    );
    let v = p.righe(&c, &sid, 0).await;
    assert!(
        v["righe"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["genere"] == "avviso" && r["corpo"]["codice"] == "finto_fallito")
    );

    let lenta = prova(2_000).await;
    let c2 = lenta.entra().await;
    let (_, sid2) = lenta.sessione(&c2).await;
    let e = lenta
        .invia(&c2, &sid2, "01LENTO000000000000000001", "lungo")
        .await;
    let turno = e["turno_id"].as_str().unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (s, t, _) = lenta
        .chiama(
            "POST",
            &format!("/api/v1/turni/{turno}/interrompi"),
            None,
            Some(&c2),
        )
        .await;
    assert_eq!(
        (s, t["stato"].clone()),
        (StatusCode::OK, json!("interrotto"))
    );

    // Provider spento: l'invio è rifiutato con provider_fermo.
    p.db.con(|k| {
        k.execute("UPDATE provider SET attivo = 0", []).unwrap();
        Ok(())
    })
    .unwrap();
    let r = p.invia(&c, &sid, "01RIFIUTO0000000000000001", "ciao").await;
    assert_eq!(r["stato"], "rifiutato");
    assert_eq!(r["rifiuto"]["codice"], "provider_fermo");
}

#[tokio::test]
async fn impostazioni_provider_e_errori_json() {
    let p = prova(5).await;
    let c = p.entra().await;
    let (s, v, _) = p
        .chiama(
            "PUT",
            "/api/v1/impostazioni",
            Some(json!({ "valori": { "tema": "scuro" } })),
            Some(&c),
        )
        .await;
    assert_eq!(
        (s, v["valori"]["tema"].clone()),
        (StatusCode::OK, json!("scuro"))
    );
    let (s, v, _) = p
        .chiama(
            "PUT",
            "/api/v1/impostazioni",
            Some(json!({ "valori": { "accesso.password_hash": "x" } })),
            Some(&c),
        )
        .await;
    assert_eq!(
        (s, v["errore"]["codice"].clone()),
        (StatusCode::FORBIDDEN, json!("chiave_riservata"))
    );
    let (_, v, _) = p
        .chiama("GET", "/api/v1/impostazioni", None, Some(&c))
        .await;
    let v = &v["valori"];
    assert_eq!(v["tema"], "scuro");
    assert!(v.get("server.segreto_cookie").is_none() && v.get("accesso.password_hash").is_none());

    let (_, v, _) = p.chiama("GET", "/api/v1/provider", None, Some(&c)).await;
    assert_eq!(v[0]["id"], "finto");
    assert_eq!(v[0]["caricato"], true);
    let (_, v, _) = p
        .chiama("GET", "/api/v1/modelli?provider=finto", None, Some(&c))
        .await;
    assert_eq!(v[0]["id"], "finto-1");
    let (s, v, _) = p.chiama("GET", "/api/v1/quote", None, Some(&c)).await;
    assert_eq!(
        (s, v[0]["provider_id"].clone()),
        (StatusCode::OK, json!("finto"))
    );

    let (s, v, _) = p
        .chiama("GET", "/api/v1/sessioni/nessuna", None, Some(&c))
        .await;
    assert_eq!(
        (s, v["errore"]["codice"].clone()),
        (StatusCode::NOT_FOUND, json!("sessione_non_trovata"))
    );
    let (s, v, _) = p
        .chiama(
            "POST",
            "/api/v1/assistenti",
            Some(json!({ "x": 1 })),
            Some(&c),
        )
        .await;
    assert_eq!(
        (s, v["errore"]["codice"].clone()),
        (StatusCode::BAD_REQUEST, json!("richiesta_non_valida"))
    );
    let (s, v, _) = p.chiama("GET", "/api/v1/non-esiste", None, Some(&c)).await;
    assert_eq!(
        (s, v["errore"]["codice"].clone()),
        (StatusCode::NOT_FOUND, json!("non_trovato"))
    );

    // Sezioni e spostamento dell'assistente.
    let (aid, sid) = p.sessione(&c).await;
    let (s, _, _) = p
        .chiama(
            "POST",
            "/api/v1/sezioni",
            Some(json!({ "id": "lavoro", "nome": "Lavoro", "ordine": 1 })),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (_, a, _) = p
        .chiama(
            "PATCH",
            &format!("/api/v1/assistenti/{aid}"),
            Some(json!({ "sezione_id": "lavoro" })),
            Some(&c),
        )
        .await;
    assert_eq!(a["sezione_id"], "lavoro");
    let (_, v, _) = p
        .chiama(
            "PATCH",
            "/api/v1/sezioni",
            Some(json!([{ "id": "lavoro", "chiusa": 1 }])),
            Some(&c),
        )
        .await;
    assert_eq!(v[0]["chiusa"], 1);
    let (_, ses, _) = p
        .chiama(
            "PATCH",
            &format!("/api/v1/sessioni/{sid}"),
            Some(json!({ "titolo": "nuovo", "archivia": true })),
            Some(&c),
        )
        .await;
    assert_eq!(
        (ses["titolo"].clone(), ses["stato"].clone()),
        (json!("nuovo"), json!("archiviata"))
    );
    let r = p
        .invia(&c, &sid, "01ARCHIVIATA00000000000001", "ciao")
        .await;
    assert_eq!(r["rifiuto"]["codice"], "sessione_archiviata");
    let (s, _, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/assistenti/{aid}/letto"),
            Some(json!({ "fino_seq": 3 })),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (_, a, _) = p
        .chiama(
            "DELETE",
            &format!("/api/v1/assistenti/{aid}"),
            None,
            Some(&c),
        )
        .await;
    assert!(a["archiviato_ms"].is_i64());
}

#[tokio::test]
async fn chiusura_del_server_chiude_i_flussi() {
    let p = prova(5).await;
    let c = p.entra().await;
    let (_, sid) = p.sessione(&c).await;
    let req = Request::builder()
        .uri(format!("/api/v1/sessioni/{sid}/flusso"))
        .header(header::COOKIE, &c)
        .body(Body::empty())
        .unwrap();
    let mut corpo = p.app.clone().oneshot(req).await.unwrap().into_body();
    let mut testo = String::new();
    assert!(leggi_fino(&mut corpo, &mut testo, "event: connected").await);
    let t = std::time::Instant::now();
    p.stato.chiudi_flussi();
    assert!(
        leggi_fino(&mut corpo, &mut testo, "event: chiusura").await,
        "{testo}"
    );
    // Poi il corpo finisce: nessun altro frame.
    let fine = tokio::time::timeout(Duration::from_secs(1), corpo.frame())
        .await
        .unwrap();
    assert!(fine.is_none(), "il flusso deve finire");
    assert!(t.elapsed() < Duration::from_secs(1));
}
