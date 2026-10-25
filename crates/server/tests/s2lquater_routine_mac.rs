//! Prove della 2.L-quater (S): una routine di un assistente Claude Code sul Mac manda al
//! lavoratore il permesso della macchina (`permesso_esecuzione`), come un invio dell'utente: il
//! lavoratore ne ricava il modo dei permessi (`always` → senza chiedere, `ask` → l'utente in chat).

mod comune;
mod s2_comune;
mod s2k_comune;

use comune::*;
use s2k_comune::*;

/// Un assistente Claude Code sul Mac con una routine; la routine «Esegui» a mano → il frame
/// `lavoro` che arriva al Mac.
async fn lavoro_della_routine(p: &Prova, c: &str, ws: &mut Socket, mid: &str, nome: &str) -> Value {
    let corpo = json!({ "nome": nome, "provider_predefinito_id": "claude_code",
                        "macchina_predefinita_id": mid });
    let (_, a) = chiama(p, c, "POST", "/api/v1/assistenti", Some(corpo)).await;
    let aid = a["id"].as_str().unwrap().to_string();
    let uri = format!("/api/v1/assistenti/{aid}/sessioni");
    let (s, v) = chiama(p, c, "POST", &uri, Some(json!({}))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let uri = format!("/api/v1/assistenti/{aid}/routine");
    let corpo = json!({"nome": "Giro", "corpo": "leggi il file", "attivatore_tipo": "ogni_secondi",
        "attivatore_json": {"every_seconds": 3600}});
    let (s, r) = chiama(p, c, "POST", &uri, Some(corpo)).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let rid = r["id"].as_str().unwrap();
    let uri = format!("/api/v1/routine/{rid}/esegui");
    let (s, v) = chiama(p, c, "POST", &uri, None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    aspetta(ws, "lavoro").await
}

async fn permesso(p: &Prova, c: &str, mid: &str, valore: &str) {
    let uri = format!("/api/v1/macchine/{mid}/permesso");
    let corpo = json!({ "permesso_esecuzione": valore });
    let (s, v) = chiama(p, c, "PUT", &uri, Some(corpo)).await;
    assert_eq!(s, StatusCode::OK, "{v}");
}

#[tokio::test]
async fn routine_sul_mac_porta_il_permesso_della_macchina() {
    let (p, c, indirizzo) = banco(None).await;
    let (mid, mut ws) = mac(&p, &c, &indirizzo).await;

    permesso(&p, &c, &mid, "always").await;
    let l = lavoro_della_routine(&p, &c, &mut ws, &mid, "Patrimonio").await;
    let r = &l["richiesta"];
    assert_eq!(r["provider_id"], "claude_code", "{l}");
    assert_eq!(r["permesso_esecuzione"], "always", "{l}");
    assert_eq!(r["chiedi_permessi"], false, "{l}");

    permesso(&p, &c, &mid, "ask").await;
    let l = lavoro_della_routine(&p, &c, &mut ws, &mid, "Patrimonio due").await;
    let r = &l["richiesta"];
    assert_eq!(r["permesso_esecuzione"], "ask", "{l}");
    assert_eq!(r["chiedi_permessi"], true, "{l}");
}
