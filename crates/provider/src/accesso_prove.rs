use super::*;

#[test]
fn account_dalle_uscite_registrate() {
    // `claude auth status` del 2026-10-25 (valori sostituiti).
    let c = r#"{"loggedIn": true, "authMethod": "claude.ai", "apiProvider": "firstParty",
        "email": "utente@esempio.it", "orgId": "x", "orgName": "Org", "subscriptionType": "max"}"#;
    assert_eq!(
        leggi_claude(c),
        Some(Account {
            nome: "utente@esempio.it".into(),
            dettaglio: Some("max".into())
        })
    );
    assert_eq!(leggi_claude(r#"{"loggedIn": false}"#), None);
    assert_eq!(leggi_claude("Not logged in"), None);

    // `agent status --format json` e il testo di `agent status`.
    let j = r#"{"status":"authenticated","isAuthenticated":true,"hasAccessToken":true,
        "hasRefreshToken":true,"userInfo":{"email":"utente@esempio.it","userId":1}}"#;
    assert_eq!(
        leggi_cursor(j).map(|a| a.nome),
        Some("utente@esempio.it".into())
    );
    assert_eq!(
        leggi_cursor("\u{2713} Logged in as utente@esempio.it\n").map(|a| a.nome),
        Some("utente@esempio.it".into())
    );
    assert_eq!(leggi_cursor(r#"{"isAuthenticated":false}"#), None);
    assert_eq!(leggi_cursor("Not logged in\n"), None);

    // `codex login status` (stderr).
    assert_eq!(
        leggi_codex("Logged in using ChatGPT\n").map(|a| a.nome),
        Some("ChatGPT".into())
    );
    assert_eq!(leggi_codex("Not logged in\n"), None);

    // `auth.json` di grok: solo le chiavi vere, valori finti.
    let g = r#"{"https://auth.x.ai::abc": {"key": "k", "auth_mode": "oidc", "email": "utente@esempio.it",
        "team_name": "Personale", "refresh_token": "r", "expires_at": 1}}"#;
    assert_eq!(
        leggi_grok(g),
        Some(Account {
            nome: "utente@esempio.it".into(),
            dettaglio: Some("Personale".into())
        })
    );
    assert_eq!(leggi_grok("{}"), None);
    assert_eq!(
        leggi_grok(r#"{"x": {"key": "k"}}"#).map(|a| a.nome),
        Some("collegato".into())
    );
}

#[test]
fn url_e_codice_nelle_righe_dei_client() {
    assert_eq!(
        url_in(
            "Open a browser and navigate to this link: https://cursor.com/loginDeepControl?challenge=a&uuid=b"
        ),
        Some("https://cursor.com/loginDeepControl?challenge=a&uuid=b".into())
    );
    assert_eq!(
        url_in("If the browser didn't open, visit: https://claude.ai/oauth/authorize?code=true"),
        Some("https://claude.ai/oauth/authorize?code=true".into())
    );
    assert_eq!(
        url_in("   https://auth.openai.com/codex/device"),
        Some("https://auth.openai.com/codex/device".into())
    );
    assert_eq!(url_in("niente"), None);
    assert_eq!(codice_in("   ABCD-12345"), Some("ABCD-12345".into()));
    assert_eq!(codice_in("  WXYZ-ABCD  "), Some("WXYZ-ABCD".into()));
    assert_eq!(codice_in("Waiting for authorization..."), None);
    assert_eq!(codice_in("https://a-b"), None);
    assert_eq!(pulisci("\u{1b}[1mciao\u{1b}[0m"), "ciao");
}

#[tokio::test]
async fn accesso_con_un_client_finto() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("prove/accesso-finto");
    let mut c = ComandoAccesso::nuovo(Path::new("/bin/sh"), &[]);
    c.argomenti = vec![p.to_string_lossy().into_owned(), "codice".into()];
    let mut a = avvia(&c, Duration::from_secs(10)).await.unwrap();
    assert_eq!(
        a.invito.url.as_deref(),
        Some("https://esempio.it/accedi?x=1")
    );
    assert_eq!(a.invito.codice_utente.as_deref(), Some("ABCD-1234"));
    let mut s = a.prendi_stdin().unwrap();
    scrivi_codice(&mut s, "il-codice").await.unwrap();
    assert_eq!(a.attendi(Duration::from_secs(10)).await, "riuscito");

    c.argomenti = vec![p.to_string_lossy().into_owned(), "lento".into()];
    let a = avvia(&c, Duration::from_secs(10)).await.unwrap();
    assert_eq!(a.attendi(Duration::from_millis(300)).await, "scaduto");

    c.argomenti = vec![p.to_string_lossy().into_owned(), "muto".into()];
    assert!(avvia(&c, Duration::from_millis(500)).await.is_err());
}
