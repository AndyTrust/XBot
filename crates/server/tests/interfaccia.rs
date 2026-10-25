//! Prove della Fase 2.C-bis: file statici alla radice e sotto /app, anteprima della barra,
//! collegamenti accesi in /io, quota del provider finto.

mod comune;

use comune::*;

async fn prendi(app: &Router, uri: &str) -> (StatusCode, String, Option<String>, Vec<u8>) {
    let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
    let r = app.clone().oneshot(req).await.unwrap();
    let tipo = r.headers()[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .to_string();
    let sw = r
        .headers()
        .get("service-worker-allowed")
        .map(|v| v.to_str().unwrap().to_string());
    let s = r.status();
    (
        s,
        tipo,
        sw,
        r.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
}

#[tokio::test]
async fn file_statici_alla_radice_e_sotto_app() {
    let p = prova(5).await;
    let web = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web");
    let app = app(p.stato.clone(), &web);
    let casi = [
        ("/", "text/html"),
        ("/stile.css", "text/css"),
        ("/app.js", "text/javascript"),
        ("/sw.js", "text/javascript"),
        ("/manifest.webmanifest", "application/manifest+json"),
        ("/icone/app.svg", "image/svg+xml"),
        ("/icone/app-192.png", "image/png"),
    ];
    for (percorso, tipo) in casi {
        for base in ["", "/app"] {
            let uri = if percorso == "/" && base == "/app" {
                "/app/".to_string()
            } else {
                format!("{base}{percorso}")
            };
            let (s, t, sw, corpo) = prendi(&app, &uri).await;
            assert_eq!(s, StatusCode::OK, "{uri}");
            assert!(t.starts_with(tipo), "{uri}: {t}");
            assert!(!corpo.is_empty(), "{uri}");
            let atteso = percorso.ends_with("sw.js").then(|| "/".to_string());
            assert_eq!(sw, atteso, "{uri}");
        }
    }
    // L'API ha la precedenza, anche sulle rotte che non esistono.
    let (s, t, _, corpo) = prendi(&app, "/api/v1/salute").await;
    assert_eq!((s, t.as_str()), (StatusCode::OK, "application/json"));
    assert_eq!(serde_json::from_slice::<Value>(&corpo).unwrap()["ok"], true);
    for uri in ["/api/v1/non-esiste", "/api/qualcosa"] {
        let (s, t, _, _) = prendi(&app, uri).await;
        assert_eq!(
            (s, t.as_str()),
            (StatusCode::NOT_FOUND, "application/json"),
            "{uri}"
        );
    }
    // Un percorso dell'interfaccia che non è un file dà index.html.
    let (s, t, _, _) = prendi(&app, "/sessione/123").await;
    assert_eq!((s, t.starts_with("text/html")), (StatusCode::OK, true));
}

#[tokio::test]
async fn anteprima_collegamenti_e_quota_del_finto() {
    let p = prova(5).await;
    let c = p.entra().await;
    let (aid, sid) = p.sessione(&c).await;
    let voce = |b: &Value| {
        b.as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s["assistenti"].as_array().unwrap().clone())
            .find(|a| a["assistente_id"] == aid.as_str())
            .unwrap()
    };
    let (_, b, _) = p.chiama("GET", "/api/v1/barra", None, Some(&c)).await;
    assert!(voce(&b)["anteprima"].is_null());
    let e = p
        .invia(&c, &sid, "01ANT0000000000000000001", "**ciao** Mario")
        .await;
    let t = e["turno_id"].as_str().unwrap().to_string();
    assert_eq!(p.aspetta_turno(&c, &t).await, "riuscito");
    let (_, b, _) = p.chiama("GET", "/api/v1/barra", None, Some(&c)).await;
    let v = voce(&b);
    // Il finto rovescia «**ciao** Mario» in «oiraM **oaic**»: l'anteprima è senza Markdown.
    assert_eq!(v["anteprima"], "oiraM oaic");
    assert!(v["anteprima_ms"].as_i64().unwrap() > 0);

    let (_, io, _) = p.chiama("GET", "/api/v1/io", None, Some(&c)).await;
    assert_eq!(io["collegamenti_accesi"], 0);

    let (_, q, _) = p.chiama("GET", "/api/v1/quote", None, Some(&c)).await;
    let finto = q
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["provider_id"] == "finto")
        .unwrap();
    assert_eq!(
        (finto["percentuale_usata"].clone(), finto["fonte"].clone()),
        (json!(27.0), json!("client"))
    );
    let fra = finto["azzera_ms"].as_i64().unwrap() - xbot_core::ora_ms();
    assert!((fra - 4 * 24 * 3600 * 1000).abs() < 60_000);
}
