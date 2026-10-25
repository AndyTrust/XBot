//! Prove dei collegamenti (Fase 2.D S2): rotte, prova con `rmcp` client su un server stdio
//! finto, permessi per strumento applicati dallo strumento `permesso`.

mod comune;
mod s2_comune;

use comune::*;
use s2_comune::*;

fn script() -> String {
    format!("{}/tests/mcp-finto.py", env!("CARGO_MANIFEST_DIR"))
}

#[tokio::test]
async fn giro_completo_dei_collegamenti() {
    let p = prova_s2(vec![claude_finto()], Some(FRASE)).await;
    let c = p.entra().await;
    let (aid, sid) = assistente(&p, &c, "Atena", "claude_code").await;

    let corpo = json!({
        "nome": "Finto MCP", "trasporto": "stdio", "comando": "python3", "argomenti": [script()],
        "ambiente": { "TOKEN_FINTO": "xyz123" }, "categoria": "prova",
    });
    let (s, v, _) = p
        .chiama("POST", "/api/v1/collegamenti", Some(corpo), Some(&c))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let id = v["id"].as_str().unwrap().to_string();

    let (_, elenco, _) = p
        .chiama("GET", "/api/v1/collegamenti", None, Some(&c))
        .await;
    let voce = &elenco["collegamenti"][0];
    assert_eq!(voce["nome"], "Finto MCP");
    assert_eq!(voce["intestazioni_presenti"], false);
    assert!(
        !elenco.to_string().contains("xyz123"),
        "i segreti non escono"
    );

    let (s, prova, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/collegamenti/{id}/prova"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{prova}");
    assert_eq!(prova["ok"], true, "{prova}");
    let strumenti = prova["strumenti"].as_array().unwrap();
    assert_eq!(strumenti.len(), 2);
    assert_eq!(strumenti[0]["sola_lettura"], true);
    assert_eq!(strumenti[0]["descrizione"], "cerca (token=xyz)");

    let perm = json!({ "permessi": { "*lettura": "sempre", "scrivi": "bloccato" } });
    let (s, v, _) = p
        .chiama(
            "PUT",
            &format!("/api/v1/collegamenti/{id}/permessi"),
            Some(perm),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, v, _) = p
        .chiama(
            "PUT",
            &format!("/api/v1/collegamenti/{id}/esclusi"),
            Some(json!({ "esclusi": [] })),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(
        v["esclusi"],
        json!([]),
        "2.L: vale per tutti, {aid} compreso"
    );
    assert_eq!(v["strumenti"][0]["permesso"], "sempre");
    assert_eq!(v["strumenti"][1]["permesso"], "bloccato");
    assert!(v["ultimo_ok_ms"].is_i64());

    // Nel turno il collegamento si chiama `finto_mcp`: i permessi per strumento valgono.
    let t = r#"[mcp:permesso {"tool_name": "mcp__finto_mcp__scrivi", "input": {}}] [mcp:permesso {"tool_name": "mcp__finto_mcp__cerca", "input": {"q": 1}}]"#;
    let (stato, testo) = turno(&p, &c, &sid, t).await;
    assert_eq!(stato, "riuscito");
    let (bloccato, cerca) = testo.split_once(" | ").unwrap();
    assert!(
        bloccato.contains("\"deny\"") && bloccato.contains("bloccato"),
        "{bloccato}"
    );
    assert_eq!(
        serde_json::from_str::<Value>(cerca).unwrap(),
        json!({ "behavior": "allow", "updatedInput": { "q": 1 } })
    );

    // Spento, il collegamento non entra più nel turno: lo strumento non è suo, quindi si chiede.
    let (s, _, _) = p
        .chiama(
            "PATCH",
            &format!("/api/v1/collegamenti/{id}"),
            Some(json!({ "abilitato": false })),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = p
        .chiama(
            "DELETE",
            &format!("/api/v1/collegamenti/{id}"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _, _) = p
        .chiama("GET", &format!("/api/v1/collegamenti/{id}"), None, Some(&c))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn errori_e_catalogo() {
    let p = prova_s2(vec![claude_finto()], None).await;
    let c = p.entra().await;
    let (s, v, _) = p
        .chiama(
            "POST",
            "/api/v1/collegamenti",
            Some(json!({ "nome": "X", "trasporto": "http" })),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    // Con la cassaforte chiusa l'ambiente non si cifra.
    let corpo =
        json!({ "nome": "X", "trasporto": "stdio", "comando": "x", "ambiente": { "A": "b" } });
    let (s, v, _) = p
        .chiama("POST", "/api/v1/collegamenti", Some(corpo), Some(&c))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["errore"]["codice"], "cassaforte_non_impostata");
    // Un comando che non esiste: la prova risponde ok=false con il motivo.
    let corpo = json!({ "nome": "Rotto", "trasporto": "stdio", "comando": "/non/esiste/mcp" });
    let (_, v, _) = p
        .chiama("POST", "/api/v1/collegamenti", Some(corpo), Some(&c))
        .await;
    let id = v["id"].as_str().unwrap();
    let (s, prova, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/collegamenti/{id}/prova"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(prova["ok"], false);
    assert!(
        prova["errore"].as_str().unwrap().contains("non parte"),
        "{prova}"
    );
    let (_, v, _) = p
        .chiama("GET", &format!("/api/v1/collegamenti/{id}"), None, Some(&c))
        .await;
    assert!(v["ultimo_errore"].is_string());
    let (s, cat, _) = p
        .chiama("GET", "/api/v1/collegamenti/catalogo", None, Some(&c))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        cat["catalogo"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["id"] == "memoria")
    );
}
