//! Prove della 2.K: catalogo completo (Postino, Google, `client_istruzioni`), prova di un
//! collegamento sul computer dove gira (lavoratore finto in-process che usa lo stesso codice
//! `xbot_provider::prova_mcp`), collegamenti divisi per macchina nei turni. OAuth in `s2k_oauth`.

mod comune;
mod s2_comune;
mod s2k_comune;

use comune::*;
use s2k_comune::*;

#[tokio::test]
async fn catalogo_completo_e_istruzioni_del_client() {
    let (p, c, _) = banco(Some("https://xbot.esempio.it/")).await;
    let (s, cat) = chiama(&p, &c, "GET", "/api/v1/collegamenti/catalogo", None).await;
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
    let voce = |id: &str| voci.iter().find(|v| v["id"] == id).unwrap().clone();
    let google = voce("google");
    assert_eq!(google["nome"], "Google (Gmail, Drive, Calendar)");
    assert_eq!(google["url"], "https://xbot.esempio.it/google-mcp/mcp");
    assert_eq!(google["richiede_oauth"], true);
    assert_eq!(google["richiede_client"], false);
    assert_eq!(google["client_nel_server"], true);
    let ist = google["client_istruzioni"].as_str().unwrap();
    assert!(
        ist.contains("https://xbot.esempio.it/google-mcp/oauth2callback"),
        "{ist}"
    );
    assert!(
        ist.contains("https://xbot.esempio.it/api/v1/oauth/ritorno"),
        "{ist}"
    );
    let slack = voce("slack");
    assert_eq!(slack["richiede_client"], true);
    let ist = slack["client_istruzioni"].as_str().unwrap();
    assert!(
        ist.contains("https://xbot.esempio.it/api/v1/oauth/ritorno"),
        "{ist}"
    );
    assert!(
        ist.contains("search:read.public") && ist.contains("users:read.email"),
        "{ist}"
    );
    for v in voci {
        assert_eq!(v["richiede_client"], v["id"] == "slack", "{v}");
        assert_eq!(
            v["client_istruzioni"].is_string(),
            ["slack", "google"].contains(&v["id"].as_str().unwrap()),
            "{v}"
        );
    }
    // Le righe: `richiede_client` e `client_istruzioni` dal catalogo; Google si riconosce dal
    // percorso anche con un'altra base.
    let sl = crea(
        &p,
        &c,
        json!({ "nome": "Slack", "trasporto": "http", "url": "https://mcp.slack.com/mcp" }),
    )
    .await;
    let go = crea(
        &p,
        &c,
        json!({ "nome": "Google", "trasporto": "http", "url": "http://altro:9/google-mcp/mcp" }),
    )
    .await;
    let (_, el) = chiama(&p, &c, "GET", "/api/v1/collegamenti", None).await;
    let riga = |id: &str| {
        el["collegamenti"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(riga(&sl)["richiede_client"], true);
    assert_eq!(riga(&sl)["richiede_oauth"], true);
    assert!(
        riga(&sl)["client_istruzioni"]
            .as_str()
            .unwrap()
            .contains("api.slack.com")
    );
    assert_eq!(riga(&go)["richiede_client"], false);
    assert_eq!(riga(&go)["client_nel_server"], true);
    assert_eq!(riga(&sl)["client_nel_server"], false);
    assert_eq!(riga(&go)["richiede_oauth"], true);
    assert!(riga(&go)["client_istruzioni"].is_string());
    let (_, uno) = chiama(&p, &c, "GET", &format!("/api/v1/collegamenti/{sl}"), None).await;
    assert_eq!(uno["richiede_client"], true);
    assert!(uno["macchina_id"].is_null());
}

#[tokio::test]
async fn prova_sul_computer_dove_gira() {
    let (p, c, indirizzo) = banco(None).await;
    let (mid, mut ws) = mac(&p, &c, &indirizzo).await;
    // Una macchina che non esiste: 400.
    let corpo = json!({ "nome": "X", "trasporto": "stdio", "comando": "python3", "macchina_id": "nessuna" });
    let (s, _) = chiama(&p, &c, "POST", "/api/v1/collegamenti", Some(corpo)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let id = crea(
        &p,
        &c,
        json!({
            "nome": "Postino finto", "trasporto": "stdio", "comando": "python3",
            "argomenti": [mcp_finto()], "ambiente": { "TOKEN_FINTO": "abcdef" }, "macchina_id": mid,
        }),
    )
    .await;
    let (_, v) = chiama(&p, &c, "GET", &format!("/api/v1/collegamenti/{id}"), None).await;
    assert_eq!(v["macchina_id"], mid.as_str());
    // La prova parte dal lavoratore: il frame porta il collegamento con l'ambiente decifrato.
    let (pp, cc, ii) = (&p, &c, &id);
    let (esito, frame) = tokio::join!(prova_coll(pp, cc, ii), rispondi_a_una_prova(&mut ws));
    assert_eq!(frame["collegamento"]["nome"], "Postino finto");
    assert_eq!(frame["collegamento"]["ambiente"][0]["nome"], "TOKEN_FINTO");
    assert_eq!(esito["ok"], true, "{esito}");
    let nomi: Vec<&str> = esito["strumenti"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["nome"].as_str().unwrap())
        .collect();
    assert_eq!(nomi, ["cerca", "scrivi"]);
    assert_eq!(esito["strumenti"][0]["descrizione"], "cerca (token=abc)");
    assert_eq!(esito["strumenti"][0]["sola_lettura"], true);
    let (_, v) = chiama(&p, &c, "GET", &format!("/api/v1/collegamenti/{id}"), None).await;
    assert!(v["ultimo_ok_ms"].is_i64(), "{v}");
    assert_eq!(v["strumenti"].as_array().unwrap().len(), 2);
    // Un errore sul computer torna così com'è.
    let uri = format!("/api/v1/collegamenti/{id}");
    chiama(
        &p,
        &c,
        "PATCH",
        &uri,
        Some(json!({ "comando": "comando-che-non-esiste-xbot" })),
    )
    .await;
    let (esito, _) = tokio::join!(prova_coll(pp, cc, ii), rispondi_a_una_prova(&mut ws));
    assert_eq!(esito["ok"], false);
    assert!(
        esito["errore"].as_str().unwrap().contains("non parte"),
        "{esito}"
    );
    // Mac spento: niente attesa, il testo lo dice.
    drop(ws);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let esito = prova_coll(&p, &c, &id).await;
    assert_eq!(esito["ok"], false);
    assert_eq!(
        esito["errore"],
        "il computer «mac» non è collegato: accendilo e riprova"
    );
    // Senza macchina (PATCH a null) la prova torna sul server.
    let corpo = json!({ "macchina_id": null, "comando": "python3" });
    let (s, v) = chiama(&p, &c, "PATCH", &uri, Some(corpo)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(v["macchina_id"].is_null());
    assert_eq!(prova_coll(&p, &c, &id).await["ok"], true);
}

#[tokio::test]
async fn collegamenti_divisi_per_macchina_nei_turni() {
    let (p, c, indirizzo) = banco(None).await;
    let (mid, mut ws) = mac(&p, &c, &indirizzo).await;
    let sul_mac = crea(&p, &c, json!({ "nome": "Postino", "trasporto": "stdio", "comando": "python3",
                                       "argomenti": ["~/strumenti/posta_mcp.py"], "macchina_id": mid })).await;
    let sul_server = crate_stdio(&p, &c, "File del server").await;
    let ovunque = crea(
        &p,
        &c,
        json!({ "nome": "Odoo", "trasporto": "http", "url": "https://o.esempio.it/mcp" }),
    )
    .await;
    let corpo = json!({ "nome": format!("Posta {}", xbot_core::nuovo_id()), "macchina_predefinita_id": mid,
                        "provider_predefinito_id": "claude_code" });
    let (s, a) = chiama(&p, &c, "POST", "/api/v1/assistenti", Some(corpo)).await;
    assert_eq!(s, StatusCode::OK, "{a}");
    let aid = a["id"].as_str().unwrap().to_string();
    for id in [&sul_mac, &sul_server, &ovunque] {
        let uri = format!("/api/v1/collegamenti/{id}/esclusi");
        let (s, v) = chiama(&p, &c, "PUT", &uri, Some(json!({ "esclusi": [] }))).await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }
    // Sul server: Postino resta fuori e lo dice.
    let srv = p.stato.servizi.macchina.id.clone();
    let (a2, s2) = (aid.clone(), srv.clone());
    let (dentro, fuori) =
        p.db.con(move |k| {
            let d = xbot_db::collegamenti::per_assistente_su(k, None, &a2, &s2, true)?;
            Ok((
                d.into_iter().map(|x| x.nome).collect::<Vec<_>>(),
                xbot_db::collegamenti::fuori_dal_turno(k, &a2, &s2, true)?,
            ))
        })
        .unwrap();
    assert_eq!(dentro, ["File del server", "Odoo"]);
    assert_eq!(fuori, [("Postino".to_string(), Some("mac".to_string()))]);
    // Sul Mac: il lavoro porta Postino e Odoo, non il collegamento stdio del server; in chat
    // l'avviso.
    let (s, sess) = chiama(
        &p,
        &c,
        "POST",
        &format!("/api/v1/assistenti/{aid}/sessioni"),
        Some(json!({})),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{sess}");
    let sid = sess["id"].as_str().unwrap().to_string();
    p.invia(&c, &sid, "i1", "le ultime due mail di lavoro")
        .await;
    let l = aspetta(&mut ws, "lavoro").await;
    assert_eq!(l["richiesta"]["provider_id"], "claude_code");
    let nomi: Vec<&str> = l["richiesta"]["collegamenti"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["nome"].as_str().unwrap())
        .collect();
    // 2.L-bis: sul filo il nome pulito del `--mcp-config`.
    assert_eq!(nomi, ["odoo", "postino"]);
    let righe = p.righe(&c, &sid, 0).await;
    let avvisi: Vec<String> = righe["righe"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["corpo"]["codice"] == "collegamento_altrove")
        .map(|r| r["corpo"]["testo"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        avvisi,
        ["il collegamento File del server gira sul server: non è in questo turno"]
    );
}

async fn crate_stdio(p: &Prova, c: &str, nome: &str) -> String {
    crea(
        p,
        c,
        json!({ "nome": nome, "trasporto": "stdio", "comando": "npx", "argomenti": ["-y", "x"] }),
    )
    .await
}

/// Solo a mano, sul Mac dell'utente: la voce Postino del catalogo provata con lo stesso codice del
/// lavoratore (`initialize` + `tools/list`, nessuna mail letta).
/// `cargo test -p xbot-server --test s2k_collegamenti -- --ignored postino`
#[tokio::test]
#[ignore]
async fn postino_sul_mac_a_mano() {
    let srv = xbot_provider::prova_mcp::ServerDaProvare {
        trasporto: "stdio".into(),
        comando: Some("python3".into()),
        argomenti: vec!["~/strumenti/posta_mcp.py".into()],
        ..Default::default()
    };
    let t = xbot_provider::prova_mcp::elenco_strumenti(&srv)
        .await
        .unwrap();
    let nomi: Vec<&str> = t.iter().map(|s| s.nome.as_str()).collect();
    println!("Postino: {nomi:?}");
    assert_eq!(nomi, ["caselle", "ultime", "leggi", "cerca", "digest"]);
    assert!(t.iter().all(|s| s.sola_lettura));
}
