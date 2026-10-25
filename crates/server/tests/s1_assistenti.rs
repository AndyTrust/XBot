//! Prove S1: assistenti (sezione 2 del contratto 2.D): avatar, blob, profilo, abilità,
//! routine con l'orologio, memoria, file, PATCH esteso, menzione.

mod aiuti_s1;
mod comune;

use aiuti_s1::*;
use comune::*;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR-finto";

#[tokio::test]
async fn avatar_blob_e_menzione() {
    let p = prova_s1(None).await;
    let c = p.entra().await;
    let (aid, _) = p.sessione(&c).await;
    let uri = format!("/api/v1/assistenti/{aid}/avatar");
    let (s, v) = multipart(&p, &uri, &c, "immagine", "a.png", "image/png", PNG).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let url = v["avatar_url"].as_str().unwrap().to_string();
    assert!(url.starts_with("/api/v1/blob/"));
    let (s, h, b) = grezzo(&p, &url, &c).await;
    assert_eq!((s, b.as_slice()), (StatusCode::OK, PNG));
    assert_eq!(h["content-type"], "image/png");
    assert!(h["cache-control"].to_str().unwrap().contains("immutable"));
    let (s, _, _) = grezzo(&p, &url, "xbot_accesso=no").await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _, _) = grezzo(&p, "/api/v1/blob/abc", &c).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // Un GIF non passa.
    let (s, v) = multipart(
        &p,
        &uri,
        &c,
        "immagine",
        "a.gif",
        "image/gif",
        b"GIF89a....",
    )
    .await;
    assert_eq!(
        (s, v["errore"]["codice"].as_str()),
        (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Some("tipo_non_accettato")
        )
    );
    // GET /assistenti: avatar_url e menzione unica.
    let (_, _, _) = p
        .chiama(
            "POST",
            "/api/v1/assistenti",
            Some(json!({"nome": "ate na"})),
            Some(&c),
        )
        .await;
    let (_, v, _) = p.chiama("GET", "/api/v1/assistenti", None, Some(&c)).await;
    let mut m: Vec<&str> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["menzione"].as_str().unwrap())
        .collect();
    m.sort();
    assert_eq!(m, ["atena", "atena2"]);
    assert!(
        v.as_array()
            .unwrap()
            .iter()
            .any(|a| a["avatar_url"] == url.as_str())
    );
    let (s, v, _) = p.chiama("DELETE", &uri, None, Some(&c)).await;
    assert_eq!((s, v["avatar_url"].is_null()), (StatusCode::OK, true));
}

#[tokio::test]
async fn profilo_abilita_e_testo_di_sistema() {
    let p = prova_s1(None).await;
    let c = p.entra().await;
    let (aid, sid) = p.sessione(&c).await;
    let (_, b, _) = p
        .chiama(
            "POST",
            "/api/v1/assistenti",
            Some(json!({"nome": "Contabile", "titolo": "Fa i conti"})),
            Some(&c),
        )
        .await;
    let base = format!("/api/v1/assistenti/{aid}");
    let (s, v, _) = p
        .chiama("GET", &format!("{base}/profilo"), None, Some(&c))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(
        v["percorso"]
            .as_str()
            .unwrap()
            .ends_with(&format!("{aid}.md"))
    );
    let (s, _, _) = p
        .chiama(
            "PUT",
            &format!("{base}/profilo"),
            Some(json!({"testo": "Sei Atena, il braccio destro."})),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK);

    let (s, v, _) = p
        .chiama(
            "POST",
            &format!("{base}/abilita"),
            Some(json!({"nome": "conti", "descrizione": "Somma", "testo": "Somma bene."})),
            Some(&c),
        )
        .await;
    assert_eq!(
        (s, v["attiva"].as_bool()),
        (StatusCode::OK, Some(true)),
        "{v}"
    );
    let (s, _, _) = p
        .chiama(
            "POST",
            &format!("{base}/abilita"),
            Some(json!({"nome": "conti"})),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _, _) = p
        .chiama(
            "POST",
            &format!("{base}/abilita"),
            Some(json!({"nome": "Non Valido"})),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    p.chiama(
        "POST",
        &format!("{base}/abilita"),
        Some(json!({"nome": "spenta", "testo": "Mai."})),
        Some(&c),
    )
    .await;
    let (s, v, _) = p
        .chiama(
            "PATCH",
            &format!("{base}/abilita/spenta"),
            Some(json!({"attiva": false})),
            Some(&c),
        )
        .await;
    assert_eq!((s, v["attiva"].as_bool()), (StatusCode::OK, Some(false)));
    let (_, v, _) = p
        .chiama("GET", &format!("{base}/abilita"), None, Some(&c))
        .await;
    assert_eq!(v["abilita"].as_array().unwrap().len(), 2);

    // Il testo di sistema: profilo, abilità attive, gli altri con @menzione, cassaforte, quaderno.
    let profili = p.stato.servizi.profili.clone();
    let t = p
        .db
        .con(move |conn| xbot_server::sistema::componi_sync(conn, &profili, &sid, Some("MEMORIA")))
        .unwrap()
        .unwrap();
    assert!(t.contains("Sei Atena"));
    assert!(t.contains("Somma bene.") && !t.contains("Mai."));
    assert!(t.contains("@contabile") && t.contains("Fa i conti"));
    assert!(t.contains("cassaforte_chiedi") && t.contains("quaderno_scrivi"));
    assert!(t.contains("MEMORIA"));
    assert_ne!(b["menzione"], "atena");

    let (s, _, _) = p
        .chiama("DELETE", &format!("{base}/abilita/spenta"), None, Some(&c))
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _, _) = p
        .chiama("DELETE", &format!("{base}/abilita/spenta"), None, Some(&c))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = p
        .chiama("GET", "/api/v1/assistenti/nessuno/profilo", None, Some(&c))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn routine_esegui_e_orologio() {
    let p = prova_s1(None).await;
    let c = p.entra().await;
    let (aid, sid) = p.sessione(&c).await;
    let uri = format!("/api/v1/assistenti/{aid}/routine");
    let corpo = json!({"nome": "Giro", "corpo": "fai il giro", "attivatore_tipo": "ogni_secondi",
        "attivatore_json": {"every_seconds": 5}});
    let (s, r, _) = p.chiama("POST", &uri, Some(corpo), Some(&c)).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let rid = r["id"].as_str().unwrap().to_string();
    assert!(r["prossima_ms"].as_i64().unwrap() > xbot_core::ora_ms());

    // «Esegui» a mano: un invio con origine routine:<id> nella sessione corrente.
    let (s, v, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/routine/{rid}/esegui"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["sessione_id"], sid.as_str());
    assert_eq!(
        p.aspetta_turno(&c, v["invio"]["turno_id"].as_str().unwrap())
            .await,
        "riuscito"
    );

    // L'orologio: la scadenza passata fa partire la routine.
    let r2 = rid.clone();
    p.db.con(move |conn| {
        conn.execute("UPDATE routine SET prossima_ms = 1 WHERE id = ?1", [&r2])
            .unwrap();
        Ok(())
    })
    .unwrap();
    assert_eq!(xbot_server::orologio::giro(&p.stato).await, 1);
    let (_, v, _) = p
        .chiama(
            "GET",
            &format!("/api/v1/routine/{rid}/esecuzioni"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(v["esecuzioni"].as_array().unwrap().len(), 2);
    let (_, r, _) = p
        .chiama("GET", &format!("/api/v1/routine/{rid}"), None, Some(&c))
        .await;
    assert!(r["prossima_ms"].as_i64().unwrap() > xbot_core::ora_ms());
    let n: i64 =
        p.db.con(|conn| {
            Ok(conn
                .query_row(
                    "SELECT COUNT(*) FROM invio WHERE origine LIKE 'routine:%'",
                    [],
                    |x| x.get(0),
                )
                .unwrap())
        })
        .unwrap();
    assert_eq!(n, 2);

    // PATCH: cron valido, poi errori.
    let (s, r, _) = p
        .chiama(
            "PATCH",
            &format!("/api/v1/routine/{rid}"),
            Some(
                json!({"attivatore_tipo": "cron", "attivatore_json": {"expression": "30 7 * * *"}}),
            ),
            Some(&c),
        )
        .await;
    assert_eq!(
        (s, r["attivatore_tipo"].as_str()),
        (StatusCode::OK, Some("cron")),
        "{r}"
    );
    let (s, _, _) = p
        .chiama(
            "PATCH",
            &format!("/api/v1/routine/{rid}"),
            Some(json!({"attivatore_json": {"expression": "boh"}})),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _, _) = p
        .chiama("POST", &uri, Some(json!({"nome": "x", "corpo": "y", "attivatore_tipo": "ogni_secondi", "attivatore_json": {"every_seconds": 1}})), Some(&c))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (_, v, _) = p.chiama("GET", &uri, None, Some(&c)).await;
    assert_eq!(v["routine"].as_array().unwrap().len(), 1);
    let (s, _, _) = p
        .chiama("DELETE", &format!("/api/v1/routine/{rid}"), None, Some(&c))
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, v, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/routine/{rid}/esegui"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(
        (s, v["errore"]["codice"].as_str()),
        (StatusCode::NOT_FOUND, Some("routine_non_trovata"))
    );
}

#[tokio::test]
async fn memoria_file_e_patch_esteso() {
    let p = prova_s1(None).await;
    let c = p.entra().await;
    let (aid, sid) = p.sessione(&c).await;
    let cartella = p.stato.servizi.cartella_dati.join("vault/Progetto");
    std::fs::create_dir_all(cartella.join("sotto")).unwrap();
    std::fs::write(cartella.join("Progetto.md"), "nota").unwrap();
    std::fs::write(cartella.join("sotto/altra.md"), "altra").unwrap();
    let patch = json!({"memoria_cartella": cartella.to_str().unwrap(), "voce_velocita": 1.2, "voce_lingua": "it",
        "colore_in_chat": false, "livello_ragionamento": "high", "provider_predefinito_id": "finto"});
    let (s, v, _) = p
        .chiama(
            "PATCH",
            &format!("/api/v1/assistenti/{aid}"),
            Some(patch),
            Some(&c),
        )
        .await;
    assert_eq!(
        (s, v["voce_lingua"].as_str()),
        (StatusCode::OK, Some("it")),
        "{v}"
    );
    let (s, v, _) = p
        .chiama(
            "PATCH",
            &format!("/api/v1/assistenti/{aid}"),
            Some(json!({"provider_predefinito_id": "boh"})),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "{v}");
    let (s, v, _) = p
        .chiama(
            "GET",
            &format!("/api/v1/assistenti/{aid}/memoria"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["note"].as_array().unwrap().len(), 2);
    assert!(v["scritture"].as_array().unwrap().is_empty());

    let (s, al) = multipart(
        &p,
        &format!("/api/v1/sessioni/{sid}/allegati"),
        &c,
        "file",
        "conti.csv",
        "text/csv",
        b"a,b\n",
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{al}");
    let (_, v, _) = p
        .chiama(
            "GET",
            &format!("/api/v1/assistenti/{aid}/file"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(v["file"][0]["nome"], "conti.csv");
    assert_eq!(v["file"][0]["blob_hash"], al["blob_hash"]);
    let (s, _, _) = p
        .chiama("GET", "/api/v1/assistenti/nessuno/file", None, Some(&c))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}
