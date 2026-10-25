//! Prove della Fase 2.D-bis (S2): caratteri di controllo, richieste ritirate e scadute, tetto
//! dei canali, variabili vietate, voce, socket, errori di `/mcp` nel formato di XBot.

mod comune;
mod s2_comune;

use comune::*;
use s2_comune::*;

async fn codice(p: &Prova, c: &str, metodo: &str, uri: &str, corpo: Value) -> (StatusCode, Value) {
    let (s, v, _) = p.chiama(metodo, uri, Some(corpo), Some(c)).await;
    (s, v["errore"]["codice"].clone())
}

#[tokio::test]
async fn caratteri_di_controllo_rifiutati_e_ripuliti() {
    let p = prova_s2(vec![claude_finto()], None).await;
    let c = p.entra().await;
    let nul = "x\u{0}y";
    let atteso = (StatusCode::BAD_REQUEST, json!("testo_non_valido"));
    assert_eq!(
        codice(&p, &c, "POST", "/api/v1/assistenti", json!({ "nome": nul })).await,
        atteso
    );
    let (aid, sid) = assistente(&p, &c, "Atena", "claude_code").await;
    let uri = format!("/api/v1/assistenti/{aid}");
    assert_eq!(
        codice(&p, &c, "PATCH", &uri, json!({ "titolo": "a\u{7}b" })).await,
        atteso
    );
    assert_eq!(
        codice(&p, &c, "PATCH", &uri, json!({ "descrizione": "riga\u{0}" })).await,
        atteso
    );
    let (s, _, _) = p
        .chiama(
            "PATCH",
            &uri,
            Some(json!({ "descrizione": "due\nrighe" })),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "a capo ammesso nella descrizione");
    assert_eq!(
        codice(&p, &c, "POST", "/api/v1/sezioni", json!({ "nome": nul })).await,
        atteso
    );
    let coll = json!({ "nome": nul, "trasporto": "stdio", "comando": "x" });
    assert_eq!(
        codice(&p, &c, "POST", "/api/v1/collegamenti", coll).await,
        atteso
    );
    assert_eq!(
        codice(&p, &c, "POST", "/api/v1/canali", json!({ "nome": nul })).await,
        atteso
    );
    let s2 = format!("/api/v1/sessioni/{sid}");
    assert_eq!(
        codice(&p, &c, "PATCH", &s2, json!({ "titolo": nul })).await,
        atteso
    );

    // Un nome con NUL scritto prima dei controlli non rompe più i turni degli altri.
    p.db.con(|conn| {
        conn.execute(
            "INSERT INTO assistente (id, nome, creato_ms, aggiornato_ms) VALUES ('vecchio', ?1, 0, 0)",
            ["a\u{0}b"],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
    let (stato, testo) = turno(&p, &c, &sid, "ciao").await;
    assert_eq!((stato.as_str(), testo.as_str()), ("riuscito", "oaic"));
}

#[tokio::test]
async fn interruzione_ritira_la_richiesta_e_chiude_lo_strumento() {
    let p = prova_s2(vec![claude_finto()], None).await;
    let c = p.entra().await;
    let (_aid, sid) = assistente(&p, &c, "Atena", "claude_code").await;
    let e = p
        .invia(&c, &sid, &xbot_core::nuovo_id(), "[permesso]")
        .await;
    let tid = e["turno_id"].as_str().unwrap().to_string();
    let r = richiesta_aperta(&p, &c).await;
    let rid = r["id"].as_str().unwrap().to_string();
    let (s, t, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/turni/{tid}/interrompi"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{t}");
    let mut stato = String::new();
    for _ in 0..300 {
        let (_, v, _) = p
            .chiama("GET", "/api/v1/richieste?stato=tutte", None, Some(&c))
            .await;
        stato = v["richieste"][0]["stato"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if stato == "ritirata" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    assert_eq!(stato, "ritirata");
    let righe = p.righe(&c, &sid, 0).await;
    let strumento = righe["righe"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["genere"] == "chiamata_strumento")
        .unwrap()
        .clone();
    assert_eq!(strumento["corpo"]["stato"], "interrotto", "{strumento}");
    assert_eq!(strumento["corpo"]["esito"], "interrotto");
    let ris = json!({ "esito": "approvata", "sempre": true });
    let (s, k) = codice(
        &p,
        &c,
        "POST",
        &format!("/api/v1/richieste/{rid}/risolvi"),
        ris,
    )
    .await;
    assert_eq!((s, k), (StatusCode::CONFLICT, json!("richiesta_chiusa")));
    let (_, perm, _) = p.chiama("GET", "/api/v1/permessi", None, Some(&c)).await;
    assert!(
        perm["permessi"].as_array().unwrap().is_empty(),
        "nessuna concessione da un turno morto"
    );
}

#[tokio::test]
async fn richiesta_scaduta() {
    let p = prova_s2(vec![claude_finto()], None).await;
    p.stato
        .motore
        .s2()
        .imposta_attesa(Duration::from_millis(500));
    let c = p.entra().await;
    let (_aid, sid) = assistente(&p, &c, "Atena", "claude_code").await;
    let (stato, testo) = turno(&p, &c, &sid, "[permesso]").await;
    assert_eq!(stato, "riuscito");
    assert_eq!(testo, "permesso negato: L'utente non ha risposto in tempo.");
    let (_, v, _) = p
        .chiama("GET", "/api/v1/richieste?stato=scaduta", None, Some(&c))
        .await;
    let rid = v["richieste"][0]["id"].as_str().unwrap().to_string();
    let (s, k) = codice(
        &p,
        &c,
        "POST",
        &format!("/api/v1/richieste/{rid}/risolvi"),
        json!({ "esito": "approvata" }),
    )
    .await;
    assert_eq!((s, k), (StatusCode::CONFLICT, json!("richiesta_chiusa")));
}

#[tokio::test]
async fn variabili_vietate_e_socket() {
    let p = prova_s2(vec![claude_finto()], Some(FRASE)).await;
    let c = p.entra().await;
    for v in [
        "PATH",
        "LD_PRELOAD",
        "DYLD_INSERT_LIBRARIES",
        "XBOT_MCP_GETTONE",
        "CLAUDE_X",
        "ANTHROPIC_API_KEY",
    ] {
        let voce = json!({ "genere": "segreto", "titolo": v, "riferimento": { "variabile": v }, "campi": { "valore": "x" } });
        let (s, k) = codice(&p, &c, "POST", "/api/v1/cassaforte", voce).await;
        assert_eq!(
            (s, k),
            (StatusCode::BAD_REQUEST, json!("variabile_non_ammessa")),
            "{v}"
        );
    }
    let voce = json!({ "genere": "segreto", "titolo": "x", "riferimento": { "variabile": "_X" }, "campi": { "valore": "x" } });
    assert_eq!(
        codice(&p, &c, "POST", "/api/v1/cassaforte", voce).await.0,
        StatusCode::BAD_REQUEST
    );
    let s2 = p.stato.motore.s2();
    assert!(s2.socket.exists());
    s2.togli_socket();
    assert!(!s2.socket.exists());
}
