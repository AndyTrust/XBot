//! Prove di integrazione della Fase 2.B: il provider `claude_code` puntato al `claude` finto,
//! la quota dal client, «tocca a te», i non letti per tempo.

mod comune;

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use comune::*;
use xbot_provider::ClaudeCode;

fn claude_finto() -> Arc<dyn Provider> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../provider/prove/claude-finto");
    let mut perm = std::fs::metadata(&p).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&p, perm).unwrap();
    let sessioni = std::env::temp_dir().join(format!("xbot-srv-{}", xbot_core::nuovo_id()));
    Arc::new(ClaudeCode::nuovo(p).con_cartella_sessioni(sessioni))
}

async fn barra_voce(p: &Prova, c: &str, aid: &str) -> Value {
    let (_, b, _) = p.chiama("GET", "/api/v1/barra", None, Some(c)).await;
    b.as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s["assistenti"].as_array().unwrap().clone())
        .find(|a| a["assistente_id"] == aid)
        .unwrap()
}

#[tokio::test]
async fn invio_con_claude_code_finto() {
    let p = prova_con(vec![claude_finto()]).await;
    let c = p.entra().await;
    let (s, a, _) = p
        .chiama(
            "POST",
            "/api/v1/assistenti",
            Some(json!({ "nome": "Prova" })),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{a}");
    let aid = a["id"].as_str().unwrap().to_string();
    let corpo = json!({ "provider_id": "claude_code", "modello_id": "claude-haiku-5-5" });
    let (s, sess, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/assistenti/{aid}/sessioni"),
            Some(corpo),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{sess}");
    let sid = sess["id"].as_str().unwrap().to_string();

    let e = p
        .invia(&c, &sid, "01CC00000000000000000001", "ciao Mario")
        .await;
    let turno = e["turno_id"].as_str().unwrap().to_string();
    assert_eq!(p.aspetta_turno(&c, &turno).await, "riuscito");

    // La risposta è la stessa riga riscritta: seq fisso, aggiornata_seq più alto.
    let righe = p.righe(&c, &sid, 0).await;
    let risposta = righe["righe"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["genere"] == "messaggio" && r["ruolo"] == "assistente")
        .unwrap()
        .clone();
    assert_eq!(risposta["corpo"]["testo"], "oiraM oaic");
    assert!(risposta["aggiornata_seq"].as_i64() > risposta["seq"].as_i64());

    // Token e costo dal `result` del campione; id esterno salvato nella sessione.
    let (tok, costo, esterna): (i64, f64, String) =
        p.db.con(|k| {
            Ok(k.query_row(
                "SELECT t.token_ingresso, t.costo_usd, s.sessione_esterna_id
                 FROM turno t JOIN sessione s ON s.id = t.sessione_id WHERE t.id = ?1",
                [&turno],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap())
        })
        .unwrap();
    assert_eq!(tok, 51_939);
    assert!((costo - 0.005_845_49).abs() < 1e-12);
    assert!(!esterna.is_empty());

    // La quota del client: seven_day è l'anello, five_hour nel dettaglio.
    let (_, q, _) = p.chiama("GET", "/api/v1/quote", None, Some(&c)).await;
    let cc = q
        .as_array()
        .unwrap()
        .iter()
        .find(|q| q["provider_id"] == "claude_code")
        .unwrap();
    assert_eq!(cc["fonte"], "client");
    assert_eq!(cc["percentuale_usata"], 24.0);
    assert_eq!(cc["azzera_ms"], 1_793_124_000_000_i64);
    assert_eq!(cc["dettaglio_json"]["five_hour"]["percentuale_usata"], 12.0);

    // «Tocca a te» e un non letto; «letto» spegne entrambi.
    let v = barra_voce(&p, &c, &aid).await;
    assert_eq!(
        (v["in_attesa_utente"].clone(), v["al_lavoro"].clone()),
        (json!(1), json!(0))
    );
    assert_eq!(v["non_letti"], 1);
    let (s, _, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/assistenti/{aid}/letto"),
            Some(json!({})),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let v = barra_voce(&p, &c, &aid).await;
    assert_eq!(
        (v["in_attesa_utente"].clone(), v["non_letti"].clone()),
        (json!(0), json!(0))
    );

    // Secondo invio: --resume con l'id di prima (il finto lo rimanda uguale).
    let e = p
        .invia(&c, &sid, "01CC00000000000000000002", "ancora")
        .await;
    let turno2 = e["turno_id"].as_str().unwrap().to_string();
    assert_eq!(p.aspetta_turno(&c, &turno2).await, "riuscito");
    let (_, s2, _) = p
        .chiama("GET", &format!("/api/v1/sessioni/{sid}"), None, Some(&c))
        .await;
    assert_eq!(s2["sessione_esterna_id"], esterna.as_str());
    let v = barra_voce(&p, &c, &aid).await;
    assert_eq!(v["in_attesa_utente"], 1);
}

#[tokio::test]
async fn claude_code_con_errore_e_strumento() {
    let p = prova_con(vec![claude_finto()]).await;
    let c = p.entra().await;
    let (aid, sid) = p.sessione(&c).await;
    let e = p
        .invia(&c, &sid, "01CC0000000000000000000F", "x [fallisci]")
        .await;
    let t = e["turno_id"].as_str().unwrap().to_string();
    assert_eq!(p.aspetta_turno(&c, &t).await, "fallito");
    let v = barra_voce(&p, &c, &aid).await;
    assert_eq!(
        v["in_attesa_utente"], 0,
        "su un fallimento niente «tocca a te»"
    );

    let e = p
        .invia(&c, &sid, "01CC0000000000000000000S", "y [strumento]")
        .await;
    let t = e["turno_id"].as_str().unwrap().to_string();
    assert_eq!(p.aspetta_turno(&c, &t).await, "riuscito");
    let righe = p.righe(&c, &sid, 0).await;
    let righe = righe["righe"].as_array().unwrap();
    let avviso = righe.iter().find(|r| r["genere"] == "avviso").unwrap();
    assert_eq!(avviso["corpo"]["codice"], "error_during_execution");
    let s = righe
        .iter()
        .find(|r| r["genere"] == "chiamata_strumento")
        .unwrap();
    assert_eq!(s["corpo"]["nome"], "Bash");
    assert_eq!(s["corpo"]["input"]["command"], "echo ciao");
    assert_eq!(s["corpo"]["stato"], "riuscito");
    assert_eq!(s["corpo"]["esito"], "ciao");
}
