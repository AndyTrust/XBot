//! Prove S2: filtri di `GET /utilizzo` e frame `richiesta`/`barra` del flusso SSE.

mod comune;
mod s2_comune;

use comune::*;
use s2_comune::*;

async fn leggi_fino(corpo: &mut Body, acc: &mut String, cerca: &str) -> bool {
    let fine = tokio::time::Instant::now() + Duration::from_secs(10);
    while !acc.contains(cerca) {
        let resto = fine.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(resto, corpo.frame()).await {
            Ok(Some(Ok(f))) => {
                if let Ok(d) = f.into_data() {
                    acc.push_str(&String::from_utf8_lossy(&d));
                }
            }
            _ => return false,
        }
    }
    true
}

#[tokio::test]
async fn utilizzo_con_filtri() {
    let p = prova_s2(
        vec![Arc::new(Finto::con_ritardo(Duration::from_millis(1)))],
        None,
    )
    .await;
    let c = p.entra().await;
    let (a1, s1) = assistente(&p, &c, "Uno", "finto").await;
    let (_a2, s2) = assistente(&p, &c, "Due", "finto").await;
    turno(&p, &c, &s1, "uno").await;
    turno(&p, &c, &s1, "uno bis").await;
    turno(&p, &c, &s2, "due").await;
    let (_, tutto, _) = p.chiama("GET", "/api/v1/utilizzo", None, Some(&c)).await;
    assert_eq!(tutto["per_provider"][0]["turni"], 3);
    let (s, v, _) = p
        .chiama(
            "GET",
            &format!("/api/v1/utilizzo?assistente_id={a1}"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["per_provider"][0]["turni"], 2);
    assert_eq!(v["per_assistente"].as_array().unwrap().len(), 1);
    assert_eq!(v["per_assistente"][0]["assistente_id"], a1.as_str());
    assert_eq!(v["per_giorno"][0]["turni"], 2);
    assert_eq!(v["filtri"]["assistente_id"], a1.as_str());
    let (_, v, _) = p
        .chiama("GET", "/api/v1/utilizzo?provider_id=finto", None, Some(&c))
        .await;
    assert_eq!(v["per_modello"][0]["turni"], 3, "{v}");
    let (_, v, _) = p
        .chiama("GET", "/api/v1/utilizzo?modello_id=nessuno", None, Some(&c))
        .await;
    assert!(v["per_modello"].as_array().unwrap().is_empty(), "{v}");
    let (_, v, _) = p
        .chiama(
            "GET",
            "/api/v1/utilizzo?provider_id=claude_code",
            None,
            Some(&c),
        )
        .await;
    for k in [
        "per_provider",
        "per_assistente",
        "per_modello",
        "per_giorno",
    ] {
        assert!(v[k].as_array().unwrap().is_empty(), "{k}: {v}");
    }
}

#[tokio::test]
async fn flusso_con_richiesta_e_barra() {
    let p = prova_s2(vec![claude_finto()], None).await;
    let c = p.entra().await;
    let (_aid, sid) = assistente(&p, &c, "Atena", "claude_code").await;
    let req = Request::builder()
        .uri(format!("/api/v1/sessioni/{sid}/flusso"))
        .header(header::COOKIE, &c)
        .body(Body::empty())
        .unwrap();
    let mut corpo = p.app.clone().oneshot(req).await.unwrap().into_body();
    let mut t = String::new();
    assert!(
        leggi_fino(&mut corpo, &mut t, "event: connected").await,
        "{t}"
    );
    p.invia(&c, &sid, &xbot_core::nuovo_id(), "[permesso]")
        .await;
    let r = richiesta_aperta(&p, &c).await;
    let id = r["id"].as_str().unwrap();
    let aperta = format!(
        "event: richiesta\ndata: {{\"id\":\"{id}\",\"stato\":\"aperta\",\"tipo\":\"revisione_comando\"}}"
    );
    assert!(leggi_fino(&mut corpo, &mut t, &aperta).await, "{t}");
    assert!(leggi_fino(&mut corpo, &mut t, "event: barra").await, "{t}");
    risolvi(&p, &c, id, "approvata").await;
    let chiusa = format!("event: richiesta\ndata: {{\"id\":\"{id}\",\"stato\":\"approvata\"");
    assert!(leggi_fino(&mut corpo, &mut t, &chiusa).await, "{t}");
    assert!(
        leggi_fino(
            &mut corpo,
            &mut t,
            "event: turno\ndata: {\"stato\":\"riuscito\""
        )
        .await,
        "{t}"
    );
    // Dopo la fine del turno arriva un altro frame `barra` («tocca a te»).
    let dopo = t.rfind("\"stato\":\"riuscito\"").unwrap();
    let mut coda = t[dopo..].to_string();
    assert!(
        leggi_fino(&mut corpo, &mut coda, "event: barra").await,
        "{t}{coda}"
    );
}
