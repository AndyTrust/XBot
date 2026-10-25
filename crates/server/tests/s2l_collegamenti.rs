//! Prove della 2.L (S): collegamenti per tutti i bot con le esclusioni, collegamenti nei lavori
//! di Grok e Cursor sul Mac, «Posta (IMAP)» nel catalogo, quota scaduta.

mod comune;
mod s2_comune;
mod s2k_comune;

use comune::*;
use s2k_comune::*;

async fn bot(p: &Prova, c: &str, provider: &str, macchina: &str) -> (String, String) {
    let corpo = json!({ "nome": format!("Bot {}", xbot_core::nuovo_id()),
                        "provider_predefinito_id": provider, "macchina_predefinita_id": macchina });
    let (s, a) = chiama(p, c, "POST", "/api/v1/assistenti", Some(corpo)).await;
    assert_eq!(s, StatusCode::OK, "{a}");
    let aid = a["id"].as_str().unwrap().to_string();
    let uri = format!("/api/v1/assistenti/{aid}/sessioni");
    let (s, sess) = chiama(p, c, "POST", &uri, Some(json!({}))).await;
    assert_eq!(s, StatusCode::OK, "{sess}");
    (aid, sess["id"].as_str().unwrap().to_string())
}

fn nomi_del_lavoro(l: &Value) -> Vec<String> {
    l["richiesta"]["collegamenti"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["nome"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn acceso_vale_per_tutti_salvo_gli_esclusi() {
    let (p, c, _) = banco(None).await;
    let srv = p.stato.servizi.macchina.id.clone();
    let (a1, _) = bot(&p, &c, "finto", &srv).await;
    let (a2, _) = bot(&p, &c, "finto", &srv).await;
    let id = crea(
        &p,
        &c,
        json!({ "nome": "Odoo", "trasporto": "http", "url": "https://o.esempio.it/mcp" }),
    )
    .await;
    let (_, v) = chiama(&p, &c, "GET", &format!("/api/v1/collegamenti/{id}"), None).await;
    assert_eq!(v["esclusi"], json!([]));
    assert!(v.get("assistenti").is_none(), "{v}");
    let usa = |aid: String| {
        let srv = srv.clone();
        p.db.con(move |k| {
            Ok(xbot_db::collegamenti::per_assistente_su(k, None, &aid, &srv, true)?.len())
        })
        .unwrap()
    };
    assert_eq!((usa(a1.clone()), usa(a2.clone())), (1, 1));
    let uri = format!("/api/v1/collegamenti/{id}/esclusi");
    let (s, v) = chiama(&p, &c, "POST", &uri, Some(json!({ "esclusi": [a2] }))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["esclusi"], json!([a2]));
    assert_eq!((usa(a1.clone()), usa(a2.clone())), (1, 0));
    let (s, v) = chiama(&p, &c, "PUT", &uri, Some(json!({ "esclusi": [] }))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(usa(a2.clone()), 1);
    let (s, _) = chiama(&p, &c, "PUT", &uri, Some(json!({ "esclusi": ["nessuno"] }))).await;
    assert!(s.is_client_error(), "{s}");
    let vecchia = format!("/api/v1/collegamenti/{id}/assistenti");
    let (s, _) = chiama(&p, &c, "PUT", &vecchia, Some(json!({ "assistenti": [] }))).await;
    assert!(
        s == StatusCode::NOT_FOUND || s == StatusCode::METHOD_NOT_ALLOWED,
        "{s}"
    );
    // Spento del tutto: per nessuno.
    let u = format!("/api/v1/collegamenti/{id}");
    chiama(&p, &c, "PATCH", &u, Some(json!({ "abilitato": false }))).await;
    assert_eq!((usa(a1), usa(a2)), (0, 0));
}

#[tokio::test]
async fn grok_e_cursor_sul_mac_ricevono_i_collegamenti() {
    let (p, c, indirizzo) = banco(None).await;
    let (mid, mut ws) = mac(&p, &c, &indirizzo).await;
    crea(
        &p,
        &c,
        json!({ "nome": "Odoo", "trasporto": "http", "url": "https://o.esempio.it/mcp",
                         "intestazioni": { "Authorization": "Bearer segreto-odoo" } }),
    )
    .await;
    crea(
        &p,
        &c,
        json!({ "nome": "Posta (IMAP)", "trasporto": "stdio", "comando": "python3",
                         "argomenti": ["~/strumenti/posta_mcp.py"], "macchina_id": mid }),
    )
    .await;
    let (_, prov) = chiama(&p, &c, "GET", "/api/v1/provider", None).await;
    for pr in prov.as_array().unwrap() {
        let id = pr["id"].as_str().unwrap();
        let atteso = ["claude_code", "grok", "cursor_agent"].contains(&id);
        assert_eq!(pr["riceve_collegamenti"], atteso, "{pr}");
    }
    for provider in ["grok", "cursor_agent"] {
        let (_, sid) = bot(&p, &c, provider, &mid).await;
        p.invia(
            &c,
            &sid,
            &xbot_core::nuovo_id(),
            "le ultime due mail dell'azienda",
        )
        .await;
        let l = aspetta(&mut ws, "lavoro").await;
        assert_eq!(l["richiesta"]["provider_id"], provider);
        assert_eq!(nomi_del_lavoro(&l), ["odoo", "posta_imap"], "{provider}");
        let odoo = &l["richiesta"]["collegamenti"][0];
        assert_eq!(odoo["intestazioni"][0]["valore"], "Bearer segreto-odoo");
        let righe = p.righe(&c, &sid, 0).await;
        let nota = righe["righe"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["corpo"]["codice"] == "permessi_della_macchina")
            .map(|r| r["corpo"]["testo"].as_str().unwrap_or_default().to_string())
            .unwrap_or_default();
        assert!(
            nota.contains("seguono il permesso della macchina"),
            "{provider}: {righe}"
        );
        // Nel carico salvato i segreti non ci sono.
        let lid = l["id"].as_str().unwrap().to_string();
        let salvato = p.db.con(move |k| xbot_db::lavori::leggi(k, &lid)).unwrap();
        assert!(!salvato.to_string().contains("segreto-odoo"));
    }
}

#[tokio::test]
async fn posta_fuori_dal_catalogo() {
    // 2026-10-25: la posta ufficiale è quella interna (`posta_*`): nessuna voce posta nel
    // catalogo, né come comando sul Mac né come server http; e niente voci tecniche.
    let (p, c, _) = banco(None).await;
    let (_, cat) = chiama(&p, &c, "GET", "/api/v1/collegamenti/catalogo", None).await;
    let voci = cat["catalogo"].as_array().unwrap();
    assert!(
        voci.iter()
            .all(|v| v["id"] != "postino" && v["id"] != "posta-imap")
    );
    for x in voci.iter() {
        assert_eq!(x["tecnico"], false, "{x}");
    }
}

#[tokio::test]
async fn quota_scaduta() {
    let (p, c, _) = banco(None).await;
    let ora = xbot_core::ora_ms();
    let dettaglio = json!({
        "five_hour": { "percentuale_usata": 80.0, "azzera_ms": ora - 60_000 },
        "seven_day": { "percentuale_usata": 30.0, "azzera_ms": ora + 3_600_000 },
    });
    p.db.con(move |k| {
        xbot_db::provider::rileva_quota(
            k,
            "finto",
            30.0,
            Some(ora - 1000),
            "client",
            Some(&dettaglio),
        )
    })
    .unwrap();
    let (s, q) = chiama(&p, &c, "GET", "/api/v1/quote", None).await;
    assert_eq!(s, StatusCode::OK);
    let f = q
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["provider_id"] == "finto")
        .unwrap()
        .clone();
    assert_eq!(f["scaduta"], true, "{f}");
    assert_eq!(f["dettaglio_json"]["five_hour"]["scaduta"], true);
    assert_eq!(f["dettaglio_json"]["seven_day"]["scaduta"], false);
    // Una lettura nuova con la finestra aperta non è scaduta.
    p.db.con(move |k| {
        xbot_db::provider::rileva_quota(k, "finto", 31.0, Some(ora + 60_000), "client", None)
    })
    .unwrap();
    let (_, q) = chiama(&p, &c, "GET", "/api/v1/quote", None).await;
    let f = q
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["provider_id"] == "finto")
        .unwrap()
        .clone();
    assert_eq!(f["scaduta"], false, "{f}");
    // La rilettura: il finto (come Claude Code) non dà la quota senza un turno.
    assert_eq!(xbot_server::quote_scadute::rileggi(&p.stato).await, 0);
}
