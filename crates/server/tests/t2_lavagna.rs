//! Prove della lavagna comune (sessioni annidate e lavagna, tappa 2): due assistenti, il `claude`
//! finto usa gli strumenti `lavagna_*` dal server interno, il conflitto sugli stessi file arriva
//! all'assistente e come avviso in chat, il testo di sistema porta cosa fanno gli altri.

mod comune;
mod s2_comune;

use s2_comune::*;

#[tokio::test]
async fn lavagna_conflitto_e_testo_di_sistema() {
    let p = prova_s2(vec![claude_finto()], None).await;
    let c = p.entra().await;
    let (_a1, s1) = assistente(&p, &c, "Alfa", "claude_code").await;
    let (_a2, s2) = assistente(&p, &c, "Beta", "claude_code").await;

    // Alfa scrive cosa fa e prenota web/app.js.
    let (stato, testo) = turno(
        &p,
        &c,
        &s1,
        r#"[mcp:lavagna_scrivi {"progetto": "xbot", "cosa": "rifaccio la barra", "file": ["web/app.js"]}]"#,
    )
    .await;
    assert_eq!(stato, "riuscito", "{testo}");
    assert!(testo.contains("rifaccio la barra"), "{testo}");
    assert!(!testo.contains("conflitti\":[{"), "{testo}");

    // Beta legge la lavagna: vede Alfa.
    let (_, testo) = turno(&p, &c, &s2, r#"[mcp:lavagna_leggi {}]"#).await;
    assert!(testo.contains("alfa") || testo.contains("Alfa"), "{testo}");
    assert!(testo.contains("web/app.js"), "{testo}");

    // Beta prenota la cartella web: conflitto con Alfa, avviso in chat.
    let (_, testo) = turno(
        &p,
        &c,
        &s2,
        r#"[mcp:lavagna_prendo_file {"file": ["web"], "cosa": "ritocco lo stile"}]"#,
    )
    .await;
    assert!(testo.contains("occupato_da"), "{testo}");
    assert!(
        testo.contains("web/app.js") || testo.contains("\"web\""),
        "{testo}"
    );
    let righe = p.righe(&c, &s2, 0).await;
    assert!(
        righe["righe"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["genere"] == "avviso" && r["corpo"]["codice"] == "lavagna_conflitto"),
        "{righe}"
    );

    // Il testo di sistema di Beta porta la voce di Alfa; quello di Alfa porta Beta.
    let (s2c, s1c) = (s2.clone(), s1.clone());
    let dir = std::env::temp_dir();
    let (per_beta, per_alfa) = p
        .stato
        .db
        .esegui(move |c| {
            Ok((
                xbot_server::sistema::componi_sync(c, &dir, &s2c, None)?,
                xbot_server::sistema::componi_sync(c, &dir, &s1c, None)?,
            ))
        })
        .await
        .unwrap();
    let (per_beta, per_alfa) = (per_beta.unwrap(), per_alfa.unwrap());
    assert!(
        per_beta.contains("Cosa fanno gli altri adesso"),
        "{per_beta}"
    );
    assert!(per_beta.contains("rifaccio la barra"), "{per_beta}");
    assert!(per_alfa.contains("ritocco lo stile"), "{per_alfa}");
    assert!(!per_alfa.contains("rifaccio la barra"), "{per_alfa}");

    // Alfa chiude: sparisce dal testo di Beta.
    turno(
        &p,
        &c,
        &s1,
        r#"[mcp:lavagna_scrivi {"finito": true, "esito": "fatto"}]"#,
    )
    .await;
    let s2c = s2.clone();
    let dir = std::env::temp_dir();
    let dopo = p
        .stato
        .db
        .esegui(move |c| xbot_server::sistema::componi_sync(c, &dir, &s2c, None))
        .await
        .unwrap()
        .unwrap();
    assert!(!dopo.contains("rifaccio la barra"), "{dopo}");
}
