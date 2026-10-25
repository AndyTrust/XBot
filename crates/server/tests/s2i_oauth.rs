//! Prove della 2.I: OAuth dei collegamenti contro `oauth-finto.py` (porta 8831), catalogo nuovo,
//! PATCH con i nomi delle intestazioni.

mod comune;
mod s2_comune;

use comune::*;
use s2_comune::*;

const PORTA: u16 = 8831;

/// Avvia il server finto e aspetta che risponda.
/// Il processo si ferma quando la prova finisce (`kill_on_drop`).
async fn finto() -> tokio::process::Child {
    let script = format!("{}/tests/oauth-finto.py", env!("CARGO_MANIFEST_DIR"));
    let figlio = tokio::process::Command::new("python3")
        .arg(script)
        .arg(PORTA.to_string())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    for _ in 0..100 {
        if reqwest::get(base("/stato")).await.is_ok() {
            return figlio;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("oauth-finto.py non risponde");
}

fn base(p: &str) -> String {
    format!("http://127.0.0.1:{PORTA}{p}")
}

async fn controllo(v: Value) {
    let r = reqwest::Client::new()
        .post(base("/controllo"))
        .json(&v)
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
}

async fn contatori() -> Value {
    reqwest::get(base("/stato"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

/// Una GET grezza: (stato, Location, corpo).
async fn ritorno(p: &Prova, uri: &str, cookie: Option<&str>) -> (StatusCode, String, String) {
    let mut r = Request::builder().method("GET").uri(uri);
    if let Some(c) = cookie {
        r = r.header(header::COOKIE, c);
    }
    let risp = p
        .app
        .clone()
        .oneshot(r.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let stato = risp.status();
    let dove = risp
        .headers()
        .get(header::LOCATION)
        .map(|v| v.to_str().unwrap().to_string())
        .unwrap_or_default();
    let corpo = risp.into_body().collect().await.unwrap().to_bytes();
    (stato, dove, String::from_utf8_lossy(&corpo).into_owned())
}

/// `avvia` e il consenso del finto: restituisce (url di autorizzazione, code, state).
async fn consenso(p: &Prova, c: &str, id: &str) -> (String, String, String) {
    let (s, v, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/collegamenti/{id}/oauth/avvia"),
            None,
            Some(c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let url = v["url"].as_str().unwrap().to_string();
    let senza = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let r = senza.get(&url).send().await.unwrap();
    assert_eq!(r.status(), 302);
    let dove = reqwest::Url::parse(r.headers()["location"].to_str().unwrap()).unwrap();
    assert_eq!(dove.path(), "/api/v1/oauth/ritorno");
    let q: std::collections::HashMap<_, _> = dove.query_pairs().into_owned().collect();
    (url, q["code"].clone(), q["state"].clone())
}

async fn prova_coll(p: &Prova, c: &str, id: &str) -> Value {
    let (s, v, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/collegamenti/{id}/prova"),
            None,
            Some(c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

async fn leggi(p: &Prova, c: &str, id: &str) -> Value {
    let (s, v, _) = p
        .chiama("GET", &format!("/api/v1/collegamenti/{id}"), None, Some(c))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    v
}

#[tokio::test]
async fn oauth_dal_collega_allo_scollega() {
    let server = finto().await;
    let p = prova_s2(vec![claude_finto()], Some(FRASE)).await;
    let c = p.entra().await;
    let (_aid, sid) = assistente(&p, &c, "Atena", "claude_code").await;
    let corpo = json!({ "nome": "Granola finta", "trasporto": "http", "url": base("/mcp"),
                        "richiede_oauth": true });
    let (s, v, _) = p
        .chiama("POST", "/api/v1/collegamenti", Some(corpo), Some(&c))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let id = v["id"].as_str().unwrap().to_string();
    let (s, _, _) = p
        .chiama(
            "PUT",
            &format!("/api/v1/collegamenti/{id}/esclusi"),
            Some(json!({ "esclusi": [] })),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let v = leggi(&p, &c, &id).await;
    assert_eq!(v["richiede_oauth"], true);
    assert_eq!(v["oauth"], Value::Null);
    let pr = prova_coll(&p, &c, &id).await;
    assert_eq!(pr["ok"], false);
    assert_eq!(pr["errore"], "serve il collegamento dell'account");

    // avvia → URL con PKCE e state; il finto rimanda al nostro ritorno.
    let (url, code, state) = consenso(&p, &c, &id).await;
    assert!(url.contains("code_challenge=") && url.contains("code_challenge_method=S256"));
    assert!(
        url.contains("redirect_uri=") && url.contains("state="),
        "{url}"
    );
    let (s, _, html) = ritorno(
        &p,
        &format!("/api/v1/oauth/ritorno?code={code}&state={state}"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(
        html.contains("Accedi a XBot"),
        "senza cookie una pagina HTML: {html}"
    );
    let (s, _, e) = ritorno(
        &p,
        "/api/v1/oauth/ritorno?code=x&state=sconosciuto",
        Some(&c),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(e.contains("oauth_stato_non_valido"), "{e}");
    let (s, dove, _) = ritorno(
        &p,
        &format!("/api/v1/oauth/ritorno?code={code}&state={state}"),
        Some(&c),
    )
    .await;
    assert_eq!(s, StatusCode::SEE_OTHER);
    assert_eq!(
        dove,
        format!("/#impostazioni/collegamenti?oauth=ok&collegamento={id}")
    );
    let v = leggi(&p, &c, &id).await;
    assert_eq!(v["oauth"]["stato"], "ok", "{v}");
    assert_eq!(v["oauth"]["account"], "utente@finto.it");
    assert_eq!(v["oauth"]["issuer"], base(""));
    let (_, tutti, _) = p
        .chiama("GET", "/api/v1/collegamenti", None, Some(&c))
        .await;
    let testo = tutti.to_string();
    for vietato in [
        "access_token",
        "refresh_token",
        "client_secret",
        "\"at-",
        "\"rt-",
    ] {
        assert!(!testo.contains(vietato), "{vietato} in {testo}");
    }
    let (_, ev, _) = p.chiama("GET", "/api/v1/eventi", None, Some(&c)).await;
    assert!(ev.to_string().contains("oauth_collegato"), "{ev}");

    // Prova verde con lo strumento `eco`.
    let pr = prova_coll(&p, &c, &id).await;
    assert_eq!(pr["ok"], true, "{pr}");
    assert_eq!(pr["strumenti"][0]["nome"], "eco");

    // Token scaduto sul server MCP (401): rinnovo forzato e secondo tentativo.
    controllo(json!({ "scadi": true, "durata": 30 })).await;
    let pr = prova_coll(&p, &c, &id).await;
    assert_eq!(pr["ok"], true, "{pr}");
    assert_eq!(contatori().await["rinnovi"], 1);
    // Ora il token dura 30 s (meno del margine di 60): si rinnova prima della prova.
    let pr = prova_coll(&p, &c, &id).await;
    assert_eq!(pr["ok"], true, "{pr}");
    assert_eq!(contatori().await["rinnovi"], 2);

    // Nel turno il collegamento entra con l'intestazione Authorization.
    let (stato, testo) = turno(&p, &c, &sid, "[mcp-elenco]").await;
    assert_eq!(stato, "riuscito");
    assert!(testo.contains("granola_finta(auth)"), "{testo}");

    // Il rinnovo fallisce: stato scaduto, fuori dal turno, avviso in chat.
    controllo(json!({ "scadi": true, "rifiuta_refresh": true })).await;
    let pr = prova_coll(&p, &c, &id).await;
    assert_eq!(pr["ok"], false, "{pr}");
    let v = leggi(&p, &c, &id).await;
    assert_eq!(v["oauth"]["stato"], "scaduto");
    assert_eq!(
        v["ultimo_errore"],
        "l'accesso è scaduto: ricollega l'account"
    );
    let (stato, testo) = turno(&p, &c, &sid, "[mcp-elenco]").await;
    assert_eq!(stato, "riuscito");
    assert!(!testo.contains("granola"), "{testo}");
    let righe = p.righe(&c, &sid, 0).await;
    assert!(
        righe.to_string().contains("collegamento_senza_accesso"),
        "{righe}"
    );

    // Un secondo avvia sostituisce il primo (stesso client registrato).
    controllo(json!({ "rifiuta_refresh": false, "durata": 3600 })).await;
    let (_, code1, state1) = consenso(&p, &c, &id).await;
    let (_, code2, state2) = consenso(&p, &c, &id).await;
    let (s, _, _) = ritorno(
        &p,
        &format!("/api/v1/oauth/ritorno?code={code1}&state={state1}"),
        Some(&c),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::BAD_REQUEST,
        "il primo tentativo non vale più"
    );
    let (s, _, _) = ritorno(
        &p,
        &format!("/api/v1/oauth/ritorno?code={code2}&state={state2}"),
        Some(&c),
    )
    .await;
    assert_eq!(s, StatusCode::SEE_OTHER);
    assert_eq!(leggi(&p, &c, &id).await["oauth"]["stato"], "ok");
    assert_eq!(contatori().await["registrazioni"], 1);

    // Scollega: revoca, token cancellati, prova rossa.
    let (s, v, _) = p
        .chiama(
            "DELETE",
            &format!("/api/v1/collegamenti/{id}/oauth"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["oauth"]["stato"], "revocato");
    assert_eq!(v["abilitato"], false);
    assert_eq!(v["richiede_oauth"], true);
    assert_eq!(contatori().await["revoche"], 1);
    let (s, _, _) = p
        .chiama(
            "DELETE",
            &format!("/api/v1/collegamenti/{id}/oauth"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(
        s,
        StatusCode::NOT_FOUND,
        "un secondo scollega non trova l'account"
    );
    let id2 = id.clone();
    let token: Option<Vec<u8>> =
        p.db.con(move |k| {
            Ok(k.query_row(
                "SELECT token_cifrato FROM oauth_account WHERE collegamento_id = ?1",
                [&id2],
                |r| r.get(0),
            )
            .unwrap())
        })
        .unwrap();
    assert!(token.is_none(), "token cancellati, non solo marcati");
    let pr = prova_coll(&p, &c, &id).await;
    assert_eq!(pr["ok"], false);
    assert_eq!(pr["errore"], "serve il collegamento dell'account");
    // Un client registrato a mano (come per Slack): `avvia` lo usa, niente registrazione.
    let corpo = json!({ "nome": "Senza registrazione", "trasporto": "http", "url": base("/mcp") });
    let (_, v, _) = p
        .chiama("POST", "/api/v1/collegamenti", Some(corpo), Some(&c))
        .await;
    let id3 = v["id"].as_str().unwrap().to_string();
    let corpo = json!({ "client_id": "cid-a-mano", "client_secret": "segreto-del-client" });
    let (s, v, _) = p
        .chiama(
            "PUT",
            &format!("/api/v1/collegamenti/{id3}/oauth/client"),
            Some(corpo),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["richiede_oauth"], true);
    assert_eq!(v["client_registrato"], true);
    assert!(!v.to_string().contains("segreto-del-client"));
    let (url, _, _) = consenso(&p, &c, &id3).await;
    assert!(url.contains("client_id=cid-a-mano"), "{url}");
    assert_eq!(contatori().await["registrazioni"], 1);
    // Il rifiuto del provider torna con il collegamento del tentativo.
    let (_, _, state3) = consenso(&p, &c, &id3).await;
    let uri = format!("/api/v1/oauth/ritorno?error=access_denied&state={state3}");
    let (s, dove, _) = ritorno(&p, &uri, Some(&c)).await;
    assert_eq!(s, StatusCode::SEE_OTHER);
    assert_eq!(
        dove,
        format!("/#impostazioni/collegamenti?oauth=errore&motivo=access_denied&collegamento={id3}")
    );
    drop(server);
}

#[tokio::test]
async fn catalogo_nuovo_e_patch_dei_nomi() {
    let p = prova_s2(vec![claude_finto()], Some(FRASE)).await;
    let c = p.entra().await;
    let (s, cat, _) = p
        .chiama("GET", "/api/v1/collegamenti/catalogo", None, Some(&c))
        .await;
    assert_eq!(s, StatusCode::OK);
    let voci = cat["catalogo"].as_array().unwrap();
    let ids: Vec<&str> = voci.iter().map(|v| v["id"].as_str().unwrap()).collect();
    // Le prime sei voci restano in testa, la prova sui file in coda; in mezzo i servizi
    // aggiunti il 2026-10-25 (WordPress, Microsoft 365, Hostinger, server ufficiali con OAuth).
    assert_eq!(
        ids[..6],
        ["memoria", "granola", "google", "slack", "notion", "github"]
    );
    assert_eq!(ids.last(), Some(&"filesystem-tmp"));
    for id in [
        "reason-network",
        "microsoft-365",
        "wordpress",
        "wordpress-com",
        "hostinger",
        "meta-ads",
        "stripe",
    ] {
        assert!(ids.contains(&id), "{id}");
    }
    for v in voci {
        assert!(v["descrizione"].is_string());
        let modo = v["richiede_oauth"] == true || v["richiede_token"] == true;
        let aperto =
            ["huggingface", "microsoft-learn", "context7"].contains(&v["id"].as_str().unwrap());
        assert!(modo || aperto || v["trasporto"] == "stdio", "{v}");
        assert!(v["categoria"] == "nostri" || v["categoria"] == "terzi");
    }
    let granola = voci.iter().find(|v| v["id"] == "granola").unwrap();
    assert_eq!(granola["url"], "https://mcp.granola.ai/mcp");
    for v in voci {
        assert_eq!(v["richiede_client"], v["id"] == "slack", "{v}");
    }
    // Una voce OAuth del catalogo nasce con `richiede_oauth` anche se il corpo non lo dice.
    let corpo =
        json!({ "nome": "Granola", "trasporto": "http", "url": "https://mcp.granola.ai/mcp" });
    let (_, v, _) = p
        .chiama("POST", "/api/v1/collegamenti", Some(corpo), Some(&c))
        .await;
    let gid = v["id"].as_str().unwrap().to_string();
    assert_eq!(leggi(&p, &c, &gid).await["richiede_oauth"], true);
    assert_eq!(leggi(&p, &c, &gid).await["client_registrato"], false);

    let corpo = json!({ "nome": "GitHub", "trasporto": "http", "url": "https://api.githubcopilot.com/mcp/",
                        "intestazioni": { "Authorization": "Bearer ghp_segreto" } });
    let (_, v, _) = p
        .chiama("POST", "/api/v1/collegamenti", Some(corpo), Some(&c))
        .await;
    let id = v["id"].as_str().unwrap().to_string();
    let patch = json!({ "intestazioni": { "X-Altro": "1" }, "istruzioni": "usa le issue" });
    let (s, v, _) = p
        .chiama(
            "PATCH",
            &format!("/api/v1/collegamenti/{id}"),
            Some(patch),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["intestazioni_nomi"], json!(["Authorization", "X-Altro"]));
    assert_eq!(v["intestazioni_presenti"], true);
    assert_eq!(v["istruzioni"], "usa le issue");
    let patch = json!({ "intestazioni": { "Authorization": null } });
    let (_, v, _) = p
        .chiama(
            "PATCH",
            &format!("/api/v1/collegamenti/{id}"),
            Some(patch),
            Some(&c),
        )
        .await;
    assert_eq!(v["intestazioni_nomi"], json!(["X-Altro"]));
    assert!(!v.to_string().contains("ghp_segreto"));
}

/// Solo a mano (rete vera, sola lettura): la scoperta di `rmcp` sui server del catalogo.
/// `cargo test -p xbot-server --test s2i_oauth -- --ignored`
#[tokio::test]
#[ignore]
async fn scoperta_sui_server_veri() {
    use rmcp::transport::AuthorizationManager;
    for (url, registra) in [
        ("https://mcp.granola.ai/mcp", true),
        ("https://mcp.notion.com/mcp", true),
        ("https://mcp.slack.com/mcp", false),
    ] {
        let g = AuthorizationManager::new(url).await.unwrap();
        let r = g.resolve_metadata().await.unwrap();
        assert!(r.source.is_discovered(), "{url}");
        assert_eq!(
            r.metadata.registration_endpoint.is_some(),
            registra,
            "{url}: {:?}",
            r.metadata.registration_endpoint
        );
        println!("{url}: issuer {:?}", r.metadata.issuer);
    }
}
