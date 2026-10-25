//! Prove di S2 (Fase 2.D): il `claude` finto legge il `--mcp-config`, passa dal ponte
//! `xbot-server mcp-interno` al socket e usa gli strumenti del server interno.

mod comune;
mod s2_comune;

use comune::*;
use s2_comune::*;

async fn invia_e_rispondi(
    p: &Prova,
    c: &str,
    sid: &str,
    testo: &str,
    esito: &str,
) -> (Value, String, String) {
    let e = p.invia(c, sid, &xbot_core::nuovo_id(), testo).await;
    let t = e["turno_id"].as_str().unwrap().to_string();
    let r = richiesta_aperta(p, c).await;
    risolvi(p, c, r["id"].as_str().unwrap(), esito).await;
    let stato = p.aspetta_turno(c, &t).await;
    (r, stato, ultima_risposta(p, c, sid).await)
}

#[tokio::test]
async fn permesso_approvato_negato_e_regola() {
    let p = prova_s2(vec![claude_finto()], None).await;
    let c = p.entra().await;
    let (_aid, sid) = assistente(&p, &c, "Atena", "claude_code").await;

    let (r, stato, testo) = invia_e_rispondi(&p, &c, &sid, "fai [permesso]", "approvata").await;
    assert_eq!(r["tipo"], "revisione_comando", "{r}");
    assert_eq!(r["carico_json"]["comando"], "echo ciao", "{r}");
    assert_eq!(r["carico_json"]["azione"], "run-command");
    assert_eq!(stato, "riuscito");
    assert_eq!(testo, "permesso concesso: echo ciao");
    // La riga `richiesta` in chat mostra l'esito.
    let righe = p.righe(&c, &sid, 0).await;
    let riga = righe["righe"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["genere"] == "richiesta")
        .unwrap()
        .clone();
    assert_eq!(riga["corpo"]["stato"], "approvata", "{riga}");

    let (_, stato, testo) = invia_e_rispondi(&p, &c, &sid, "fai [permesso]", "negata").await;
    assert_eq!(stato, "riuscito");
    assert!(
        testo.starts_with("permesso negato: L'utente ha negato"),
        "{testo}"
    );

    // Una regola di revisione «blocca» decide senza chiedere.
    let regole = json!({ "regole": [{ "tipo": "blocca", "testo": "echo ciao", "attiva": true }] });
    let (s, v, _) = p
        .chiama("PUT", "/api/v1/regole-revisione", Some(regole), Some(&c))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (stato, testo) = turno(&p, &c, &sid, "fai [permesso]").await;
    assert_eq!(stato, "riuscito");
    assert!(
        testo.contains("Bloccato dalla regola di revisione"),
        "{testo}"
    );

    // La cartella del turno (mcp-config) non resta sul disco.
    let turni = p.stato.motore.s2().cartella.join("turni");
    let rimaste = std::fs::read_dir(&turni).map(|d| d.count()).unwrap_or(0);
    assert_eq!(
        rimaste,
        0,
        "cartelle dei turni rimaste in {}",
        turni.display()
    );
}

#[tokio::test]
async fn concessione_stabile_con_sempre() {
    let p = prova_s2(vec![claude_finto()], None).await;
    let c = p.entra().await;
    let (_aid, sid) = assistente(&p, &c, "Atena", "claude_code").await;
    let e = p
        .invia(&c, &sid, &xbot_core::nuovo_id(), "[permesso]")
        .await;
    let r = richiesta_aperta(&p, &c).await;
    let corpo = json!({ "esito": "approvata", "sempre": true });
    let (s, v, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/richieste/{}/risolvi", r["id"].as_str().unwrap()),
            Some(corpo),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    p.aspetta_turno(&c, e["turno_id"].as_str().unwrap()).await;
    // La seconda volta passa senza richiesta.
    let (stato, testo) = turno(&p, &c, &sid, "[permesso]").await;
    assert_eq!(
        (stato.as_str(), testo.as_str()),
        ("riuscito", "permesso concesso: echo ciao")
    );
}

#[tokio::test]
async fn strumenti_interni_ambiente_e_cassaforte() {
    let p = prova_s2(vec![claude_finto()], Some(FRASE)).await;
    let c = p.entra().await;
    let (aid, sid) = assistente(&p, &c, "Atena", "claude_code").await;
    let (_, _sid_b) = assistente(&p, &c, "Ceo AI", "claude_code").await;

    // Un segreto concesso all'assistente arriva nell'ambiente del processo, mai in chat.
    let voce = json!({
        "genere": "segreto", "titolo": "Token prova", "riferimento": { "variabile": "TOKEN_PROVA" },
        "campi": { "valore": "abc123" }, "assistenti": [aid],
    });
    let (s, v, _) = p
        .chiama("POST", "/api/v1/cassaforte", Some(voce), Some(&c))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let vid = v["id"].as_str().unwrap().to_string();
    let (_, testo) = turno(&p, &c, &sid, "[ambiente:TOKEN_PROVA] [ambiente:ALTRO]").await;
    assert_eq!(testo, "TOKEN_PROVA presente (6 caratteri) | ALTRO assente");

    let (_, testo) = turno(&p, &c, &sid, "[mcp:assistenti_elenco]").await;
    assert!(
        testo.contains("@atena") && testo.contains("@ceoai"),
        "{testo}"
    );
    let (_, testo) = turno(&p, &c, &sid, "[mcp:cassaforte_elenco]").await;
    assert!(
        testo.contains("Token prova") && !testo.contains("abc123"),
        "{testo}"
    );

    // cassaforte_chiedi: richiesta di tipo cassaforte, valore in un file privato del turno.
    let chiedi = format!(r#"[mcp:cassaforte_chiedi {{"id": "{vid}", "motivo": "prova"}}]"#);
    let (r, stato, testo) = invia_e_rispondi(&p, &c, &sid, &chiedi, "approvata").await;
    assert_eq!(r["tipo"], "cassaforte");
    assert_eq!(r["carico_json"]["variabile"], "TOKEN_PROVA", "{r}");
    assert_eq!(stato, "riuscito");
    assert!(
        testo.starts_with("Approvato. Il valore è nel file"),
        "{testo}"
    );
    assert!(!testo.contains("abc123"));
    let file = testo
        .split("nel file ")
        .nth(1)
        .unwrap()
        .split(" (variabile")
        .next()
        .unwrap();
    assert!(
        !std::path::Path::new(file).exists(),
        "il file del valore resta dopo il turno: {file}"
    );
    // la ricerca viaggia nel corpo del POST
    let (st, r, _) = p
        .chiama(
            "POST",
            "/api/v1/cassaforte/cerca",
            Some(json!({ "testo": "token" })),
            Some(&c),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["voci"].as_array().unwrap().len(), 1, "{r}");
    let (_, u, _) = p
        .chiama(
            "GET",
            &format!("/api/v1/cassaforte/{vid}/usi"),
            None,
            Some(&c),
        )
        .await;
    assert!(
        u["usi"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["azione"] == "variabile_ambiente"),
        "{u}"
    );

    // quaderno_scrivi senza quaderno.py della macchina scrive comunque (ripiego sul file).
    let (_, testo) = turno(
        &p,
        &c,
        &sid,
        r#"[mcp:quaderno_scrivi {"agente": "prova-s2-quaderno", "testo": "riga di prova"}]"#,
    )
    .await;
    assert!(
        testo.contains("prova-s2-quaderno") || testo.starts_with("Salvato"),
        "{testo}"
    );
}
