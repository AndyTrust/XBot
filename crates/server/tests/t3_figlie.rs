//! Prove delle sessioni figlie (sessioni annidate e lavagna, tappa 3): un assistente apre una
//! figlia con `apri_figlia`; il padre vede una riga di stato che cambia e, alla fine, il
//! risultato; le richieste di permesso della figlia compaiono nella riga del padre.

mod comune;
mod s2_comune;

use std::time::Duration;

use comune::*;
use s2_comune::*;

/// La riga `evento` della figlia nel padre.
async fn riga_figlia(p: &Prova, c: &str, padre: &str) -> Option<Value> {
    let r = p.righe(c, padre, 0).await;
    r["righe"]
        .as_array()?
        .iter()
        .find(|x| x["genere"] == "evento" && x["corpo"]["codice"] == "figlia")
        .map(|x| x["corpo"].clone())
}

async fn aspetta_stato(p: &Prova, c: &str, padre: &str, stato: &str) -> Value {
    for _ in 0..100 {
        if let Some(k) = riga_figlia(p, c, padre).await
            && k["stato"] == stato
        {
            return k;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!(
        "la figlia non è arrivata a «{stato}»: {:?}",
        riga_figlia(p, c, padre).await
    );
}

#[tokio::test]
async fn figlia_finisce_e_riporta_al_padre() {
    let p = prova_s2(vec![claude_finto()], None).await;
    let c = p.entra().await;
    let (aid, padre) = assistente(&p, &c, "Alfa", "claude_code").await;

    let (stato, testo) = turno(
        &p,
        &c,
        &padre,
        r#"[mcp:apri_figlia {"compito": "cerca le novita", "titolo": "ricerca"}]"#,
    )
    .await;
    assert_eq!(stato, "riuscito", "{testo}");
    assert!(testo.contains("Chat figlia aperta"), "{testo}");

    let k = aspetta_stato(&p, &c, &padre, "finita").await;
    assert_eq!(k["titolo"], "ricerca");
    assert!(!k["riassunto"].as_str().unwrap_or("").is_empty(), "{k}");

    // Il risultato è arrivato al padre come testo di un pari (ruolo collega) e l'ha fatto rispondere.
    let mut visto = false;
    for _ in 0..100 {
        let r = p.righe(&c, &padre, 0).await;
        if r["righe"].as_array().unwrap().iter().any(|x| {
            x["genere"] == "messaggio"
                && x["ruolo"] == "collega"
                && x["corpo"]["testo"]
                    .as_str()
                    .is_some_and(|t| t.contains("è finita"))
        }) {
            visto = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(visto, "il risultato non è risalito al padre");

    // La chat principale resta quella del padre; la figlia è nell'elenco con il suo padre.
    let aid2 = aid.clone();
    let corrente: String = p
        .stato
        .db
        .esegui(move |c| {
            Ok(c.query_row(
                "SELECT sessione_corrente_id FROM stato_lettura WHERE assistente_id = ?1",
                [&aid2],
                |r| r.get(0),
            )
            .unwrap())
        })
        .await
        .unwrap();
    assert_eq!(corrente, padre);
}

#[tokio::test]
async fn permesso_della_figlia_compare_nel_padre() {
    let p = prova_s2(vec![claude_finto()], None).await;
    let c = p.entra().await;
    let (_aid, padre) = assistente(&p, &c, "Alfa", "claude_code").await;

    turno(
        &p,
        &c,
        &padre,
        // Le parentesi in escape: il `claude` finto del padre non deve vedere la parola chiave.
        r#"[mcp:apri_figlia {"compito": "fai \u005bpermesso\u005d", "titolo": "scrittura"}]"#,
    )
    .await;
    let k = aspetta_stato(&p, &c, &padre, "in_attesa").await;
    let rid = k["richieste"][0]["id"]
        .as_str()
        .expect("richiesta nella riga del padre")
        .to_string();
    risolvi(&p, &c, &rid, "approvata").await;
    let k = aspetta_stato(&p, &c, &padre, "finita").await;
    assert_eq!(k["richieste"].as_array().unwrap().len(), 0, "{k}");
}

#[tokio::test]
async fn utente_apre_una_figlia_dalla_rotta_e_legge_la_lavagna() {
    let p = prova_s2(vec![claude_finto()], None).await;
    let c = p.entra().await;
    let (_a, padre) = assistente(&p, &c, "Alfa", "claude_code").await;
    let (_b, _sb) = assistente(&p, &c, "Beta", "claude_code").await;

    let (s, v, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/sessioni/{padre}/figlie"),
            Some(json!({ "compito": "controlla i log", "titolo": "log", "menzione": "@beta" })),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let fid = v["figlia_id"].as_str().unwrap().to_string();
    aspetta_stato(&p, &c, &padre, "finita").await;

    let (_, v, _) = p
        .chiama(
            "GET",
            &format!("/api/v1/sessioni/{padre}/figlie"),
            None,
            Some(&c),
        )
        .await;
    assert_eq!(v["figlie"][0]["id"], fid.as_str(), "{v}");
    assert_eq!(v["figlie"][0]["figlia_stato"], "finita");

    // Un assistente che non c'è: errore leggibile, nessuna figlia in più.
    let (s, _, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/sessioni/{padre}/figlie"),
            Some(json!({ "compito": "x", "menzione": "@nessuno" })),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (s, v, _) = p.chiama("GET", "/api/v1/lavagna", None, Some(&c)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(v["voci"].is_array(), "{v}");
}
