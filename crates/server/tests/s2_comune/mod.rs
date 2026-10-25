//! Aiuti delle prove di S2 (Fase 2.D): server con cassaforte, `claude` finto, attese.
#![allow(dead_code)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use crate::comune::*;
use xbot_provider::ClaudeCode;
use xbot_server::servizi::OpzioniServizi;

pub const FRASE: &str = "frase-della-cassaforte-di-prova";

pub fn claude_finto() -> Arc<dyn Provider> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../provider/prove/claude-finto");
    let mut perm = std::fs::metadata(&p).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&p, perm).unwrap();
    let sessioni = std::env::temp_dir().join(format!("xbot-s2-{}", xbot_core::nuovo_id()));
    Arc::new(ClaudeCode::nuovo(p).con_cartella_sessioni(sessioni))
}

/// Un server in memoria con i servizi della 2.D (cassaforte aperta se c'è la frase) e il
/// ponte MCP puntato al binario `xbot-server` compilato.
pub async fn prova_s2(provider: Vec<Arc<dyn Provider>>, frase: Option<&str>) -> Prova {
    let db = Db::apri(":memory:").unwrap();
    let segreto = base64::engine::general_purpose::STANDARD.encode([9u8; 32]);
    let stato = Stato::prepara_servizi(
        db.clone(),
        provider,
        Some(SegretoTesto::nuovo(segreto)),
        false,
        Vec::new(),
        OpzioniServizi::per_prove(frase),
    )
    .await
    .unwrap();
    stato
        .motore
        .s2()
        .imposta_programma(env!("CARGO_BIN_EXE_xbot-server"));
    stato.motore.s2().imposta_attesa(Duration::from_secs(20));
    let url_totp = accesso::impianta(
        &db,
        &stato.chiave_totp,
        "Mario",
        &SegretoTesto::nuovo(PASSWORD),
        true,
    )
    .unwrap()
    .unwrap();
    Prova {
        app: app(stato.clone(), Path::new("web-che-non-esiste")),
        stato,
        db,
        url_totp,
    }
}

/// Assistente con una sessione sul provider dato; restituisce (assistente, sessione).
pub async fn assistente(p: &Prova, c: &str, nome: &str, provider: &str) -> (String, String) {
    let (s, a, _) = p
        .chiama(
            "POST",
            "/api/v1/assistenti",
            Some(json!({ "nome": nome })),
            Some(c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{a}");
    let aid = a["id"].as_str().unwrap().to_string();
    let corpo = json!({ "provider_id": provider });
    let (s, sess, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/assistenti/{aid}/sessioni"),
            Some(corpo),
            Some(c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{sess}");
    (aid, sess["id"].as_str().unwrap().to_string())
}

/// Aspetta (al massimo 15 s) una richiesta aperta e la restituisce.
pub async fn richiesta_aperta(p: &Prova, c: &str) -> Value {
    for _ in 0..300 {
        let (_, v, _) = p
            .chiama("GET", "/api/v1/richieste?stato=aperta", None, Some(c))
            .await;
        if let Some(r) = v["richieste"].as_array().and_then(|a| a.first()) {
            return r.clone();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("nessuna richiesta aperta");
}

pub async fn risolvi(p: &Prova, c: &str, id: &str, esito: &str) {
    let (s, v, _) = p
        .chiama(
            "POST",
            &format!("/api/v1/richieste/{id}/risolvi"),
            Some(json!({ "esito": esito })),
            Some(c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
}

/// Il testo dell'ultima bolla dell'assistente nella sessione.
pub async fn ultima_risposta(p: &Prova, c: &str, sid: &str) -> String {
    let r = p.righe(c, sid, 0).await;
    r["righe"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|x| x["genere"] == "messaggio" && x["ruolo"] == "assistente")
        .map(|x| x["corpo"]["testo"].as_str().unwrap_or_default().to_string())
        .unwrap_or_default()
}

/// Invia e aspetta la fine del turno; restituisce (stato del turno, ultima risposta).
pub async fn turno(p: &Prova, c: &str, sid: &str, testo: &str) -> (String, String) {
    let e = p.invia(c, sid, &xbot_core::nuovo_id(), testo).await;
    let t = e["turno_id"]
        .as_str()
        .unwrap_or_else(|| panic!("{e}"))
        .to_string();
    let stato = p.aspetta_turno(c, &t).await;
    (stato, ultima_risposta(p, c, sid).await)
}
