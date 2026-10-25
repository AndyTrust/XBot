//! Prove S1: richieste e permessi (sezione 4), macchine, utilizzo, versione, eventi, push
//! (sezione 8), allegati e spostamento (sezione 9).

mod aiuti_s1;
mod comune;

use aiuti_s1::*;
use comune::*;
use xbot_core::TipoRichiesta;
use xbot_server::richieste;

#[tokio::test]
async fn richiesta_attesa_risolta_e_sempre() {
    let p = prova_s1(None).await;
    let c = p.entra().await;
    let (assistente, sid) = p.sessione(&c).await;
    let carico =
        json!({"azione": "run-command", "comando": "ls -la", "descrizione": "elenca i file"});
    let r = richieste::crea(&p.stato, &sid, TipoRichiesta::PermessoLocale, carico, None)
        .await
        .unwrap();
    let id = r["id"].as_str().unwrap().to_string();
    let (s, v, _) = p.chiama("GET", "/api/v1/richieste", None, Some(&c)).await;
    assert_eq!(
        (s, v["richieste"][0]["id"].as_str()),
        (StatusCode::OK, Some(id.as_str()))
    );

    // Chi aspetta si sveglia quando l'utente risponde.
    let (stato, id2) = (p.stato.clone(), id.clone());
    let attesa =
        tokio::spawn(async move { richieste::attendi(&stato, &id2, Duration::from_secs(5)).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let corpo = json!({"esito": "approvata", "sempre": true, "comando_approvato": "ls -la"});
    let (s, v, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/richieste/{id}/risolvi"),
            Some(corpo.clone()),
            Some(&c),
        )
        .await;
    assert_eq!(
        (s, v["stato"].as_str()),
        (StatusCode::OK, Some("approvata")),
        "{v}"
    );
    let ris = attesa.await.unwrap().unwrap();
    assert!(ris.approvata() && ris.sempre);
    assert_eq!(ris.comando_approvato.as_deref(), Some("ls -la"));

    // Due volte: 409. La riga in chat mostra lo stato.
    let (s, v, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/richieste/{id}/risolvi"),
            Some(corpo),
            Some(&c),
        )
        .await;
    assert_eq!(
        (s, v["errore"]["codice"].as_str()),
        (StatusCode::CONFLICT, Some("richiesta_gia_risolta"))
    );
    let righe = p.righe(&c, &sid, 0).await;
    let riga = righe["righe"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["genere"] == "richiesta")
        .unwrap()
        .clone();
    assert_eq!(riga["corpo"]["stato"], "approvata");
    let (s, _, _) = p
        .chiama(
            "POST",
            "/api/v1/richieste/nessuna/risolvi",
            Some(json!({"esito": "negata"})),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/richieste/{id}/risolvi"),
            Some(json!({"esito": "forse"})),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // «Sempre» ha scritto la concessione stabile.
    let (_, v, _) = p.chiama("GET", "/api/v1/permessi", None, Some(&c)).await;
    let perm = v["permessi"][0].clone();
    assert_eq!(
        (
            perm["azione"].as_str(),
            perm["risorsa"].as_str(),
            perm["concesso"].as_str()
        ),
        (Some("run-command"), Some("ls -la"), Some("sempre"))
    );
    let trovato =
        p.db.con(move |conn| {
            xbot_db::permessi::cerca(
                conn,
                "run-command",
                None,
                Some("ls -la"),
                Some(&assistente),
                None,
            )
        })
        .unwrap();
    assert_eq!(trovato.as_deref(), Some("sempre"));
    let (s, _, _) = p
        .chiama(
            "DELETE",
            &format!("/api/v1/permessi/{}", perm["id"].as_str().unwrap()),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _, _) = p
        .chiama(
            "DELETE",
            &format!("/api/v1/permessi/{}", perm["id"].as_str().unwrap()),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Scadenza: senza risposta la richiesta diventa «scaduta».
    let r = richieste::crea(
        &p.stato,
        &sid,
        TipoRichiesta::Cassaforte,
        json!({"motivo": "prova"}),
        None,
    )
    .await
    .unwrap();
    let ris = richieste::attendi(
        &p.stato,
        r["id"].as_str().unwrap(),
        Duration::from_millis(100),
    )
    .await
    .unwrap();
    assert_eq!(ris.esito, xbot_core::StatoRichiesta::Scaduta);
    let (_, v, _) = p
        .chiama("GET", "/api/v1/richieste?stato=scaduta", None, Some(&c))
        .await;
    assert_eq!(v["richieste"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn regole_di_revisione() {
    let p = prova_s1(None).await;
    let c = p.entra().await;
    let regole = json!({"regole": [{"tipo": "blocca", "testo": "niente sudo"}, {"tipo": "consenti", "testo": "git status", "attiva": false}]});
    let (s, v, _) = p
        .chiama("PUT", "/api/v1/regole-revisione", Some(regole), Some(&c))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, v, _) = p
        .chiama("GET", "/api/v1/regole-revisione", None, Some(&c))
        .await;
    assert_eq!(v["regole"].as_array().unwrap().len(), 2);
    assert_eq!(v["regole"][1]["attiva"], 0);
    let troppe: Vec<Value> = (0..21)
        .map(|i| json!({"tipo": "blocca", "testo": format!("r{i}")}))
        .collect();
    let (s, _, _) = p
        .chiama(
            "PUT",
            "/api/v1/regole-revisione",
            Some(json!({"regole": troppe})),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let lunga = json!({"regole": [{"tipo": "blocca", "testo": "x".repeat(1001)}]});
    let (s, _, _) = p
        .chiama("PUT", "/api/v1/regole-revisione", Some(lunga), Some(&c))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn macchine_utilizzo_versione_eventi() {
    let p = prova_s1(None).await;
    let c = p.entra().await;
    let (_, sid) = p.sessione(&c).await;
    let v = p
        .invia(&c, &sid, "01J0000000000000000000INV1", "ciao")
        .await;
    assert_eq!(
        p.aspetta_turno(&c, v["turno_id"].as_str().unwrap()).await,
        "riuscito"
    );

    let (s, v, _) = p.chiama("GET", "/api/v1/macchine", None, Some(&c)).await;
    assert_eq!(s, StatusCode::OK);
    let m = v["macchine"][0].clone();
    let mid = m["id"].as_str().unwrap().to_string();
    assert_eq!(
        (m["stato"].as_str(), m["radici"].is_array()),
        (Some("connessa"), true)
    );
    assert_eq!(mid, p.stato.servizi.macchina.id);
    let (s, v, _) = p
        .chiama(
            "GET",
            &format!("/api/v1/macchine/{mid}/lavori?limite=5"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["turni"][0]["stato"], "riuscito");
    assert!(v["turni"][0]["durata_ms"].is_i64());
    let (s, v, _) = p
        .chiama(
            "PUT",
            &format!("/api/v1/macchine/{mid}/permesso"),
            Some(json!({"permesso_esecuzione": "always"})),
            Some(&c),
        )
        .await;
    assert_eq!(
        (s, v["permesso_esecuzione"].as_str()),
        (StatusCode::OK, Some("always"))
    );
    let (_, v, _) = p
        .chiama(
            "GET",
            &format!("/api/v1/macchine/{mid}/permesso"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(v["permesso_esecuzione"], "always");
    let (s, _, _) = p
        .chiama(
            "PUT",
            &format!("/api/v1/macchine/{mid}/permesso"),
            Some(json!({"permesso_esecuzione": "boh"})),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, v, _) = p
        .chiama("GET", "/api/v1/macchine/nessuna/lavori", None, Some(&c))
        .await;
    assert_eq!(
        (s, v["errore"]["codice"].as_str()),
        (StatusCode::NOT_FOUND, Some("macchina_non_trovata"))
    );
    let (s, v, _) = p
        .chiama("POST", "/api/v1/macchine/biglietto", None, Some(&c))
        .await;
    assert_eq!(s, StatusCode::OK);
    let b = v["biglietto"].as_str().unwrap().to_string();
    assert!(
        v["comando"]
            .as_str()
            .unwrap()
            .starts_with("xbot-worker enroll ")
            && v["comando"].as_str().unwrap().ends_with(&b)
    );
    let h = xbot_server::accesso::hash_token(&b);
    assert!(
        p.db.con(move |conn| xbot_db::macchine::biglietto_valido(conn, &h))
            .unwrap()
    );

    let (s, v, _) = p.chiama("GET", "/api/v1/utilizzo", None, Some(&c)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["per_provider"][0]["provider_id"], "finto");
    assert_eq!(v["per_provider"][0]["turni"], 1);
    assert_eq!(v["per_assistente"][0]["nome"], "Atena");
    assert_eq!(v["per_giorno"].as_array().unwrap().len(), 1);
    let (_, v, _) = p
        .chiama("GET", "/api/v1/utilizzo?da_ms=0&a_ms=1", None, Some(&c))
        .await;
    assert!(v["per_provider"].as_array().unwrap().is_empty());
    let (s, _, _) = p
        .chiama("GET", "/api/v1/utilizzo?da_ms=x", None, Some(&c))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (s, v, _) = p.chiama("GET", "/api/v1/versione", None, Some(&c)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["schema"], xbot_db::VERSIONE_SCHEMA.to_string());
    assert!(v["commit"].is_string() && v["compilato_ms"].is_i64() && v["avviato_ms"].is_i64());
    assert_eq!(v["macchina"]["id"], mid.as_str());
    p.db.con(|conn| xbot_db::eventi::scrivi(conn, "prova", &json!({"x": 1})))
        .unwrap();
    let (s, v, _) = p
        .chiama("GET", "/api/v1/eventi?limite=1", None, Some(&c))
        .await;
    assert_eq!(
        (s, v["eventi"][0]["tipo"].as_str()),
        (StatusCode::OK, Some("prova"))
    );
    let (s, _, _) = p.chiama("GET", "/api/v1/versione", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn push_chiave_abbonamento_e_notifiche() {
    let p = prova_s1(None).await;
    let c = p.entra().await;
    let (s, v, _) = p.chiama("GET", "/api/v1/push/chiave", None, Some(&c)).await;
    assert_eq!(
        (s, v["errore"]["codice"].as_str()),
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Some("push_non_configurato")
        )
    );
    let ab = json!({"endpoint": "https://push.example/abc",
        // Chiavi di prova generate per questo test (una P-256 qualsiasi), nessun abbonamento vero.
        "chiavi": {"p256dh": "BMMKgNnBWr0dmSotV2PIn_x2uhLITyt8i9hepEcwB7hT6ykJFYmWDsU8Hdioe6hrL6A2UpvCXHP_buOD3-sg2ek", "auth": "2h7lgiIZCXhSFYWWUObThQ"}, // gitleaks:allow
        "dispositivo": "S24"});
    let (s, v, _) = p
        .chiama(
            "POST",
            "/api/v1/push/abbonamento",
            Some(ab.clone()),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v2, _) = p
        .chiama("POST", "/api/v1/push/abbonamento", Some(ab), Some(&c))
        .await;
    assert_eq!((s, &v2["id"]), (StatusCode::OK, &v["id"]));
    let (s, _, _) = p
        .chiama(
            "POST",
            "/api/v1/push/abbonamento",
            Some(json!({"endpoint": "http://x"})),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Il notificatore vede «tocca a te», la richiesta e l'avviso, e li salva in `notifica`.
    let da = xbot_core::ora_ms() - 1;
    let (_, sid) = p.sessione(&c).await;
    let v = p
        .invia(&c, &sid, "01J0000000000000000000INV2", "ciao")
        .await;
    p.aspetta_turno(&c, v["turno_id"].as_str().unwrap()).await;
    let v = p
        .invia(&c, &sid, "01J0000000000000000000INV3", "[fallisci]")
        .await;
    p.aspetta_turno(&c, v["turno_id"].as_str().unwrap()).await;
    richieste::crea(
        &p.stato,
        &sid,
        TipoRichiesta::Domanda,
        json!({"domanda": "va bene?"}),
        None,
    )
    .await
    .unwrap();
    let a = xbot_core::ora_ms() + 1;
    let n =
        p.db.con(move |conn| xbot_server::push::da_mandare(conn, da, a))
            .unwrap();
    let tipi: Vec<&str> = n.iter().map(|x| x.tipo).collect();
    assert!(
        tipi.contains(&"tocca_a_te") && tipi.contains(&"richiesta") && tipi.contains(&"avviso"),
        "{tipi:?}"
    );
    let cliente = reqwest::Client::new();
    for x in n {
        assert_eq!(
            xbot_server::push::notifica(&p.stato, &cliente, x)
                .await
                .unwrap(),
            0
        );
    }
    let salvate = p.db.con(xbot_db::notifiche::non_lette).unwrap();
    assert!(salvate.len() >= 3);
    let (_, v, _) = p
        .chiama(
            "DELETE",
            "/api/v1/push/abbonamento",
            Some(json!({"endpoint": "https://push.example/abc"})),
            Some(&c),
        )
        .await;
    assert_eq!(v["tolti"], 1);
}

#[tokio::test]
async fn allegati_al_provider_e_sposta() {
    let p = prova_s1(None).await;
    let c = p.entra().await;
    let (_, sid) = p.sessione(&c).await;
    let (s, al) = multipart(
        &p,
        &format!("/api/v1/sessioni/{sid}/allegati"),
        &c,
        "file",
        "foto.png",
        "image/png",
        b"\x89PNGdati",
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{al}");
    let aid = al["id"].as_str().unwrap().to_string();
    let (s, _) = multipart(
        &p,
        "/api/v1/sessioni/nessuna/allegati",
        &c,
        "file",
        "x.txt",
        "text/plain",
        b"x",
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = multipart(
        &p,
        &format!("/api/v1/sessioni/{sid}/allegati"),
        &c,
        "altro",
        "x.txt",
        "text/plain",
        b"x",
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Nell'invio: l'id dell'allegato diventa il percorso della copia con il nome; un percorso
    // scritto dal client si scarta.
    let uri = format!("/api/v1/sessioni/{sid}/invii");
    let corpo = json!({"id": "01J0000000000000000000INV4", "testo": "guarda", "allegati": [aid, "/etc/passwd", {"id": aid}]});
    let (s, v, _) = p.chiama("POST", &uri, Some(corpo), Some(&c)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    p.aspetta_turno(&c, v["turno_id"].as_str().unwrap()).await;
    let percorsi =
        xbot_server::sistema::allegati_del_turno(&p.db, &sid, "01J0000000000000000000INV4").await;
    assert_eq!(percorsi.len(), 2);
    assert!(percorsi[0].ends_with("foto.png") && percorsi[0].is_file());

    // Sposta: solo verso una macchina connessa.
    p.db.con(|conn| {
        xbot_db::macchine::registra_se_stessa(conn, "mac-utente", "mac", "mac", "macos", "arm64")
    })
    .unwrap();
    let (s, v, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/sessioni/{sid}/sposta"),
            Some(json!({"macchina_id": "mac-utente"})),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["sessione"]["macchina_id"], "mac-utente");
    assert!(v["sessione"]["sessione_esterna_id"].is_null());
    let riassunto = v["riassunto_passaggio"].as_str().unwrap();
    assert!(
        riassunto.contains("Mario: guarda") && riassunto.contains("Atena: adraug"),
        "{riassunto}"
    );
    let profili = p.stato.servizi.profili.clone();
    let s2 = sid.clone();
    let t =
        p.db.con(move |conn| xbot_server::sistema::componi_sync(conn, &profili, &s2, None))
            .unwrap()
            .unwrap();
    assert!(t.contains("Dove eravamo") && t.contains("Mario: guarda"));
    p.db.con(|conn| xbot_db::macchine::ferma(conn, "mac-utente"))
        .unwrap();
    let (s, v, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/sessioni/{sid}/sposta"),
            Some(json!({"macchina_id": "mac-utente"})),
            Some(&c),
        )
        .await;
    assert_eq!(
        (s, v["errore"]["codice"].as_str()),
        (StatusCode::CONFLICT, Some("macchina_non_connessa"))
    );
    let (s, _, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/sessioni/{sid}/sposta"),
            Some(json!({"macchina_id": "boh"})),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}
