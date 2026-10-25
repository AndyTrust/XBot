//! Prove della voce (Fase 2.D S2): `/trascrivi` con uno script finto, `/sintesi` contro un
//! fish.audio finto (402 sul modello a pagamento → modello gratuito; 402 sempre → credito
//! finito), normalizzazione «tutto per intero», `/voci`.

mod comune;
mod s2_comune;

use std::path::PathBuf;

use axum::routing::post;
use comune::*;
use s2_comune::*;
use xbot_server::voce::ConfigVoce;

/// Un fish.audio finto: 402 con `s2.1-pro`, 200 con il gratuito e nel corpo il testo ricevuto.
async fn fish_finto() -> String {
    async fn tts(
        h: axum::http::HeaderMap,
        axum::Json(v): axum::Json<Value>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        if h.get("authorization").and_then(|x| x.to_str().ok()) != Some("Bearer chiave-finta") {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        if h.get("model").and_then(|x| x.to_str().ok()) != Some("s2.1-pro-free") {
            return StatusCode::PAYMENT_REQUIRED.into_response();
        }
        let corpo = format!(
            "MP3|{}|{}|{}",
            v["reference_id"].as_str().unwrap(),
            v["format"],
            v["text"].as_str().unwrap()
        );
        ([(header::CONTENT_TYPE, "audio/mpeg")], corpo).into_response()
    }
    async fn sempre_402() -> StatusCode {
        StatusCode::PAYMENT_REQUIRED
    }
    let app = Router::new()
        .route("/v1/tts", post(tts))
        .route("/rotto", post(sempre_402));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ind = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    format!("http://{ind}")
}

fn config(fish: &str) -> ConfigVoce {
    let qui = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    ConfigVoce {
        python: "python3".into(),
        trascrivi: qui.join("tests/trascrivi-finto.py"),
        normalizza: qui.join("../../voce/normalizza.py"),
        fish_url: format!("{fish}/v1/tts"),
        fish_voce: Some("voce-xbot".into()),
        fish_modello: "s2.1-pro".into(),
        fish_chiave: Some(SegretoTesto::nuovo("chiave-finta")),
    }
}

async fn manda(
    p: &Prova,
    c: &str,
    metodo: &str,
    uri: &str,
    tipo: &str,
    corpo: Vec<u8>,
) -> (StatusCode, Vec<u8>) {
    let req = Request::builder()
        .method(metodo)
        .uri(uri)
        .header(header::COOKIE, c)
        .header(header::CONTENT_TYPE, tipo)
        .body(Body::from(corpo))
        .unwrap();
    let r = p.app.clone().oneshot(req).await.unwrap();
    let s = r.status();
    (
        s,
        r.into_body().collect().await.unwrap().to_bytes().to_vec(),
    )
}

#[tokio::test]
async fn trascrivi_sintesi_e_voci() {
    let p = prova_s2(vec![Arc::new(Finto::nuovo())], None).await;
    let c = p.entra().await;
    let fish = fish_finto().await;
    p.stato.motore.s2().imposta_voce(config(&fish));

    let b = "XXconfineXX";
    let mut corpo = format!("--{b}\r\nContent-Disposition: form-data; name=\"lingua\"\r\n\r\nit\r\n--{b}\r\nContent-Disposition: form-data; name=\"audio\"; filename=\"a.wav\"\r\nContent-Type: audio/wav\r\n\r\n").into_bytes();
    corpo.extend(vec![0u8; 1000]);
    corpo.extend(format!("\r\n--{b}--\r\n").into_bytes());
    let (s, r) = manda(
        &p,
        &c,
        "POST",
        "/api/v1/trascrivi",
        &format!("multipart/form-data; boundary={b}"),
        corpo,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&r));
    let v: Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(
        v,
        json!({ "testo": "ciao Mario (1000 byte, it)", "durata_ms": 3000, "motore": "faster-whisper", "modello": "finto" })
    );

    // Oltre 25 MB: 413. Un file che non è audio: 400 audio_non_valido, senza percorsi.
    let mut grande = format!(
        "--{b}\r\nContent-Disposition: form-data; name=\"audio\"; filename=\"g.wav\"\r\n\r\n"
    )
    .into_bytes();
    grande.extend(vec![0u8; 27 * 1024 * 1024]);
    grande.extend(format!("\r\n--{b}--\r\n").into_bytes());
    let (s, r) = manda(
        &p,
        &c,
        "POST",
        "/api/v1/trascrivi",
        &format!("multipart/form-data; boundary={b}"),
        grande,
    )
    .await;
    assert_eq!(
        s,
        StatusCode::PAYLOAD_TOO_LARGE,
        "{}",
        String::from_utf8_lossy(&r)
    );
    let mut cattivo = config(&fish);
    cattivo.trascrivi = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/trascrivi-finto.py");
    cattivo.python = "python3".into();
    p.stato.motore.s2().imposta_voce(cattivo);
    let mut testo_finto = format!("--{b}\r\nContent-Disposition: form-data; name=\"lingua\"\r\n\r\nrotto\r\n--{b}\r\nContent-Disposition: form-data; name=\"audio\"; filename=\"a.txt\"\r\n\r\n").into_bytes();
    testo_finto.extend(b"non sono audio");
    testo_finto.extend(format!("\r\n--{b}--\r\n").into_bytes());
    let (s, r) = manda(
        &p,
        &c,
        "POST",
        "/api/v1/trascrivi",
        &format!("multipart/form-data; boundary={b}"),
        testo_finto,
    )
    .await;
    let v: Value = serde_json::from_slice(&r).unwrap();
    assert_eq!(
        (s, v["errore"]["codice"].as_str()),
        (StatusCode::BAD_REQUEST, Some("audio_non_valido")),
        "{v}"
    );
    assert!(
        !v.to_string().contains('/'),
        "nessun percorso nel messaggio: {v}"
    );
    p.stato.motore.s2().imposta_voce(config(&fish));

    let testo = json!({ "testo": "Alle 16:30 la VPS fa +70%" })
        .to_string()
        .into_bytes();
    let (s, audio) = manda(
        &p,
        &c,
        "POST",
        "/api/v1/sintesi",
        "application/json",
        testo.clone(),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&audio));
    assert_eq!(
        String::from_utf8(audio).unwrap(),
        "MP3|voce-xbot|\"mp3\"|Alle sedici e mezza il server privato virtuale fa più settanta per cento"
    );

    // 402 anche sul modello gratuito: credito finito.
    let mut rotto = config(&fish);
    rotto.fish_url = format!("{fish}/rotto");
    p.stato.motore.s2().imposta_voce(rotto);
    let (s, r) = manda(
        &p,
        &c,
        "POST",
        "/api/v1/sintesi",
        "application/json",
        testo.clone(),
    )
    .await;
    assert_eq!(s, StatusCode::PAYMENT_REQUIRED);
    assert_eq!(
        serde_json::from_slice::<Value>(&r).unwrap()["errore"]["codice"],
        "credito_voce_finito"
    );

    // Senza chiave: voce non configurata.
    let mut senza = config(&fish);
    senza.fish_chiave = None;
    p.stato.motore.s2().imposta_voce(senza);
    let (s, r) = manda(&p, &c, "POST", "/api/v1/sintesi", "application/json", testo).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        serde_json::from_slice::<Value>(&r).unwrap()["errore"]["codice"],
        "voce_non_configurata"
    );

    let (s, v, _) = p
        .chiama(
            "PUT",
            "/api/v1/impostazioni",
            Some(json!({ "valori": { "voce.elenco": [{ "id": "v2", "nome": "Sara" }] } })),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (_, v, _) = p.chiama("GET", "/api/v1/voci", None, Some(&c)).await;
    assert_eq!(
        v,
        json!({ "voci": [{ "id": "voce-xbot", "nome": "XBot" }, { "id": "v2", "nome": "Sara" }] })
    );

    let (s, v, _) = p
        .chiama(
            "POST",
            "/api/v1/sintesi",
            Some(json!({ "testo": " " })),
            Some(&c),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
}
