//! Prove dei canali (Fase 2.D S2): token, server MCP Streamable HTTP su `/mcp` con un client
//! `rmcp` vero, registro delle chiamate, revoca e rigenerazione.

mod comune;
mod s2_comune;

use comune::*;
use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use s2_comune::*;

async fn servi(p: &Prova) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ind = l.local_addr().unwrap();
    let app = p.app.clone();
    tokio::spawn(async move {
        axum::serve(
            l,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    format!("http://{ind}")
}

fn testo_di(r: &rmcp::model::CallToolResult) -> String {
    serde_json::to_value(&r.content).unwrap()[0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

async fn stato_http(base: &str, token: Option<&str>) -> u16 {
    let mut r = reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }));
    if let Some(t) = token {
        r = r.bearer_auth(t);
    }
    r.send().await.unwrap().status().as_u16()
}

#[tokio::test]
async fn canale_con_client_rmcp() {
    let p = prova_s2(
        vec![Arc::new(Finto::con_ritardo(Duration::from_millis(2)))],
        None,
    )
    .await;
    let c = p.entra().await;
    let (aid, _sid) = assistente(&p, &c, "Atena", "finto").await;
    let corpo = json!({ "nome": "grok-bot", "assistente_id": aid, "strumenti": ["elenco_assistenti", "invia_a_assistente", "stato_quote"] });
    let (s, k, _) = p
        .chiama("POST", "/api/v1/canali", Some(corpo), Some(&c))
        .await;
    assert_eq!(s, StatusCode::OK, "{k}");
    let (kid, token) = (
        k["id"].as_str().unwrap().to_string(),
        k["token"].as_str().unwrap().to_string(),
    );
    assert!(token.starts_with("plc_"));
    assert_eq!(k["url"], "/mcp");
    let (_, elenco, _) = p.chiama("GET", "/api/v1/canali", None, Some(&c)).await;
    assert!(
        !elenco.to_string().contains(&token),
        "il token si vede una volta sola"
    );

    let base = servi(&p).await;
    let cfg = StreamableHttpClientTransportConfig::with_uri(format!("{base}/mcp"))
        .auth_header(token.clone());
    let client = ().serve(StreamableHttpClientTransport::from_config(cfg)).await.unwrap();
    let strumenti = client.list_all_tools().await.unwrap();
    let mut nomi: Vec<String> = strumenti.iter().map(|t| t.name.to_string()).collect();
    nomi.sort();
    assert_eq!(
        nomi,
        vec!["elenco_assistenti", "invia_a_assistente", "stato_quote"]
    );

    let r = client
        .call_tool(CallToolRequestParams::new("elenco_assistenti"))
        .await
        .unwrap();
    assert!(testo_di(&r).contains("@atena"), "{}", testo_di(&r));

    let mut a = serde_json::Map::new();
    a.insert("testo".into(), json!("ciao"));
    a.insert("attendi_secondi".into(), json!(10));
    let r = client
        .call_tool(CallToolRequestParams::new("invia_a_assistente").with_arguments(a))
        .await
        .unwrap();
    let v: Value = serde_json::from_str(&testo_di(&r)).unwrap();
    assert_eq!(v["stato"], "riuscito", "{v}");
    let atteso: String = "Il canale «grok-bot» ti scrive: ciao"
        .chars()
        .rev()
        .collect();
    assert_eq!(v["risposta"], atteso.as_str());

    // Tetto orario: con limite 1 il secondo invio dell'ora è negato (429 troppe_chiamate).
    let (st, k, _) = p
        .chiama(
            "PATCH",
            &format!("/api/v1/canali/{kid}"),
            Some(json!({ "limite_orario": 1 })),
            Some(&c),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{k}");
    assert_eq!(k["limite_orario"], 1);
    let mut a = serde_json::Map::new();
    a.insert("testo".into(), json!("ancora"));
    let r = client
        .call_tool(CallToolRequestParams::new("invia_a_assistente").with_arguments(a))
        .await
        .unwrap();
    assert_eq!(r.is_error, Some(true));
    assert!(
        testo_di(&r).starts_with("429 troppe_chiamate"),
        "{}",
        testo_di(&r)
    );

    let r = client
        .call_tool(CallToolRequestParams::new("cassaforte_elenco"))
        .await
        .unwrap();
    assert_eq!(r.is_error, Some(true));
    assert!(testo_di(&r).contains("non è concesso"));
    let _ = client.cancel().await;

    let (_, ch, _) = p
        .chiama(
            "GET",
            &format!("/api/v1/canali/{kid}/chiamate?limite=10"),
            None,
            Some(&c),
        )
        .await;
    let esiti: Vec<&str> = ch["chiamate"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["esito"].as_str().unwrap())
        .collect();
    assert_eq!(esiti, vec!["negato", "negato", "ok", "ok"], "{ch}");
    let (_, elenco, _) = p.chiama("GET", "/api/v1/canali", None, Some(&c)).await;
    assert_eq!(elenco["canali"][0]["chiamate_oggi"], 4);

    // Un corpo che rmcp rifiuta in testo semplice arriva nel formato di XBot.
    let r = reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .bearer_auth(&token)
        .header("content-type", "text/plain")
        .body("non json")
        .send()
        .await
        .unwrap();
    assert!(r.status().is_client_error());
    let v: Value = r.json().await.unwrap();
    assert!(v["errore"]["codice"].is_string(), "{v}");

    // Senza token o con un token sbagliato: 401. Rigenera: il vecchio non vale più. Revoca.
    assert_eq!(stato_http(&base, None).await, 401);
    assert_eq!(stato_http(&base, Some("plc_sbagliato")).await, 401);
    let (_, n, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/canali/{kid}/rigenera"),
            None,
            Some(&c),
        )
        .await;
    let nuovo = n["token"].as_str().unwrap().to_string();
    assert_eq!(stato_http(&base, Some(&token)).await, 401);
    assert_eq!(stato_http(&base, Some(&nuovo)).await, 200);
    let (s, _, _) = p
        .chiama("DELETE", &format!("/api/v1/canali/{kid}"), None, Some(&c))
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert_eq!(stato_http(&base, Some(&nuovo)).await, 401);
    let (s, v, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/canali/{kid}/rigenera"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(v["errore"]["codice"], "canale_revocato");
}

/// 2026-10-25: n8n-xbot fa partire le routine di XBot dal canale e ne legge l'esito.
#[tokio::test]
async fn routine_dal_canale() {
    let p = prova_s2(
        vec![Arc::new(Finto::con_ritardo(Duration::from_millis(2)))],
        None,
    )
    .await;
    let c = p.entra().await;
    let (aid, _sid) = assistente(&p, &c, "Patrimonio", "finto").await;
    let (s, r, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/assistenti/{aid}/routine"),
            Some(json!({ "nome": "Riunione patrimonio", "corpo": "fai il punto",
                         "attivatore_tipo": "cron", "attivatore_json": { "expression": "30 6 * * *" } })),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let rid = r["id"].as_str().unwrap().to_string();
    let (s, r2, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/assistenti/{aid}/routine"),
            Some(json!({ "nome": "Riunione spenta", "corpo": "niente", "abilitata": false,
                         "attivatore_tipo": "cron", "attivatore_json": { "expression": "0 7 * * *" } })),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r2}");
    let (s, k, _) = p
        .chiama(
            "POST",
            "/api/v1/canali",
            Some(json!({ "nome": "n8n-xbot", "assistente_id": null, "strumenti": ["esegui_routine", "esito_routine"] })),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{k}");
    let token = k["token"].as_str().unwrap().to_string();

    let base = servi(&p).await;
    let cfg =
        StreamableHttpClientTransportConfig::with_uri(format!("{base}/mcp")).auth_header(token);
    let client = ().serve(StreamableHttpClientTransport::from_config(cfg)).await.unwrap();
    let mut nomi: Vec<String> = client
        .list_all_tools()
        .await
        .unwrap()
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    nomi.sort();
    assert_eq!(nomi, vec!["esegui_routine", "esito_routine"]);

    // Per pezzo di nome, senza maiuscole.
    let mut a = serde_json::Map::new();
    a.insert("routine".into(), json!("riunione patr"));
    let r = client
        .call_tool(CallToolRequestParams::new("esegui_routine").with_arguments(a))
        .await
        .unwrap();
    assert_ne!(r.is_error, Some(true), "{}", testo_di(&r));
    let v: Value = serde_json::from_str(&testo_di(&r)).unwrap();
    assert_eq!(v["routine_id"], rid.as_str());
    assert_eq!(v["stato"], "accettato", "{v}");
    let invio_id = v["invio_id"].as_str().unwrap().to_string();
    assert!(v["esecuzione_id"].as_str().is_some_and(|x| !x.is_empty()));

    let mut a = serde_json::Map::new();
    a.insert("invio_id".into(), json!(invio_id));
    a.insert("attendi_secondi".into(), json!(10));
    let r = client
        .call_tool(CallToolRequestParams::new("esito_routine").with_arguments(a))
        .await
        .unwrap();
    let v: Value = serde_json::from_str(&testo_di(&r)).unwrap();
    assert_eq!(v["stato"], "riuscito", "{v}");
    let atteso: String = "fai il punto".chars().rev().collect();
    assert_eq!(v["risposta"], atteso.as_str());

    // L'esecuzione è registrata come quelle dell'orologio.
    let (_, es, _) = p
        .chiama(
            "GET",
            &format!("/api/v1/routine/{rid}/esecuzioni"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(es["esecuzioni"].as_array().unwrap().len(), 1, "{es}");

    // Disabilitata, sconosciuta, ambigua.
    for (cercata, pezzo) in [
        ("Riunione spenta", "disabilitata"),
        ("non esiste", "nessuna routine"),
        ("riunione", "ambiguo"),
    ] {
        let mut a = serde_json::Map::new();
        a.insert("routine".into(), json!(cercata));
        let r = client
            .call_tool(CallToolRequestParams::new("esegui_routine").with_arguments(a))
            .await
            .unwrap();
        assert_eq!(r.is_error, Some(true), "{cercata}: {}", testo_di(&r));
        assert!(testo_di(&r).contains(pezzo), "{cercata}: {}", testo_di(&r));
    }
    // Un canale senza lo strumento non lo vede e non lo usa: come per cassaforte_elenco.
    let r = client
        .call_tool(CallToolRequestParams::new("invia_a_assistente"))
        .await
        .unwrap();
    assert_eq!(r.is_error, Some(true));
}
