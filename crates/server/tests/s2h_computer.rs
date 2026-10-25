//! Fase 2.H: il computer degli assistenti con un desktop finto (`desktop-finto.py`): accendi
//! idempotente, stato in `/assistenti` e `/squadra`, schermo dal proxy WebSocket (handshake RFB
//! ed eco), terminale dal proxy HTTP, processo morto → `errore`, `ricrea`, `spegni`; il proxy
//! del terminale verso un ttyd finto in-process (WebSocket con eco).

mod comune;

use comune::*;
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use xbot_server::computer::Programmi;

struct Banco {
    _turno: tokio::sync::MutexGuard<'static, ()>,
    p: Prova,
    cookie: String,
    indirizzo: String,
    _x11: std::path::PathBuf,
}

/// I posti (display e porte) sono gli stessi per ogni prova: una prova alla volta.
static UNA_ALLA_VOLTA: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn banco(muori: Option<(&str, &str)>) -> Banco {
    let turno = UNA_ALLA_VOLTA.lock().await;
    let p = prova(0).await;
    let cookie = p.entra().await;
    let x11 = std::env::temp_dir().join(format!("xbot-x11-{}", xbot_core::nuovo_id()));
    std::fs::create_dir_all(&x11).unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/desktop-finto.py");
    let mut prog = Programmi::finti(&script, &x11);
    if let Some((k, v)) = muori {
        prog.ambiente.push((k.into(), v.into()));
    }
    p.stato.motore.computer().imposta_programmi(prog);
    let ascolto = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let indirizzo = ascolto.local_addr().unwrap().to_string();
    let app = p.app.clone();
    tokio::spawn(async move { axum::serve(ascolto, app).await.unwrap() });
    Banco {
        _turno: turno,
        p,
        cookie,
        indirizzo,
        _x11: x11,
    }
}

impl Banco {
    async fn chiama(&self, metodo: &str, uri: &str) -> (StatusCode, Value) {
        let (s, v, _) = self.p.chiama(metodo, uri, None, Some(&self.cookie)).await;
        (s, v)
    }

    async fn assistente(&self, nome: &str) -> String {
        let (s, v, _) = self
            .p
            .chiama(
                "POST",
                "/api/v1/assistenti",
                Some(json!({ "nome": nome })),
                Some(&self.cookie),
            )
            .await;
        assert_eq!(s, StatusCode::OK, "{v}");
        v["id"].as_str().unwrap().to_string()
    }

    fn richiesta_ws(&self, percorso: &str, protocollo: &str) -> Request<()> {
        let mut r = format!("ws://{}{percorso}", self.indirizzo)
            .into_client_request()
            .unwrap();
        r.headers_mut()
            .insert(header::COOKIE, self.cookie.parse().unwrap());
        r.headers_mut()
            .insert("sec-websocket-protocol", protocollo.parse().unwrap());
        r
    }
}

#[tokio::test]
async fn accendi_schermo_terminale_spegni() {
    let b = banco(None).await;
    let id = b.assistente("Schermo").await;
    let base = format!("/api/v1/assistenti/{id}/computer");
    let (s, v) = b.chiama("GET", &base).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["stato"], "spento");
    assert_eq!(v["schermo"], format!("{base}/schermo"));
    assert_eq!(v["terminale"], format!("{base}/terminale/"));
    // Lo schermo di un computer spento non si apre.
    assert!(
        tokio_tungstenite::connect_async(b.richiesta_ws(&format!("{base}/schermo"), "binary"))
            .await
            .is_err()
    );

    let (s, v) = b.chiama("POST", &format!("{base}/accendi")).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["stato"], "acceso", "{v}");
    assert_eq!(v["display"], 50);
    assert_eq!(v["larghezza"], 1280);
    assert!(v["acceso_ms"].as_i64().is_some());
    // Idempotente.
    let (_, v2) = b.chiama("POST", &format!("{base}/accendi")).await;
    assert_eq!(v2["acceso_ms"], v["acceso_ms"]);
    // Lo stato sta anche in /assistenti e /squadra.
    let (_, a) = b.chiama("GET", "/api/v1/assistenti").await;
    let a = a
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == id)
        .unwrap()
        .clone();
    assert_eq!(a["computer"]["stato"], "acceso");
    let (_, q) = b.chiama("GET", "/api/v1/squadra").await;
    let q = q["assistenti"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == id)
        .unwrap()
        .clone();
    assert_eq!(q["computer"]["stato"], "acceso");
    // La casa è privata.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let casa = b.p.stato.motore.computer().casa(&id);
        let m = std::fs::metadata(&casa).unwrap().permissions().mode();
        assert_eq!(m & 0o777, 0o700);
        assert!(casa.join("chrome").is_dir());
    }

    // Schermo: handshake RFB dal proxy, poi eco.
    let (mut ws, risp) =
        tokio_tungstenite::connect_async(b.richiesta_ws(&format!("{base}/schermo"), "binary"))
            .await
            .unwrap();
    assert_eq!(
        risp.headers().get("sec-websocket-protocol").unwrap(),
        "binary"
    );
    let primo = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(primo.into_data().as_ref(), b"RFB 003.008\n");
    ws.send(Message::binary(b"RFB 003.008\n".to_vec()))
        .await
        .unwrap();
    let eco = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(eco.into_data().as_ref(), b"RFB 003.008\n");
    drop(ws);

    // Terminale: la pagina di ttyd arriva col percorso intero (base-path).
    let r = reqwest_get(&b, &format!("{base}/terminale/")).await;
    assert_eq!(r.0, 200);
    assert_eq!(r.1, format!("ttyd finto GET {base}/terminale/"));
    // Senza accesso niente.
    let (s, _, _) =
        b.p.chiama("GET", &format!("{base}/terminale/"), None, None)
            .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Per i turni: DISPLAY e porta di Chrome.
    let (display, _, porta) = b.p.stato.motore.computer().per_turno(&id).unwrap();
    assert_eq!(display, ":50");
    assert_eq!(porta, 9350);

    // Un secondo assistente prende il posto dopo.
    let id2 = b.assistente("Secondo").await;
    let (_, v) = b
        .chiama(
            "POST",
            &format!("/api/v1/assistenti/{id2}/computer/accendi"),
        )
        .await;
    assert_eq!(v["stato"], "acceso", "{v}");
    assert_eq!(v["display"], 51);

    // Spegni: tutto chiuso, posto libero, volontà salvata.
    let (s, v) = b.chiama("POST", &format!("{base}/spegni")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["stato"], "spento");
    assert_eq!(v["display"], Value::Null);
    assert!(b.p.stato.motore.computer().posto(&id).is_none());
    let x11 = b._x11.join("X50");
    for _ in 0..50 {
        if !x11.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!x11.exists(), "Xvfb finto ancora vivo");
    let (_, a) = b.chiama("GET", &format!("/api/v1/assistenti/{id}")).await;
    assert_eq!(a["computer_acceso"], 0);
    let r = reqwest_get(&b, &format!("{base}/terminale/")).await;
    assert_eq!(r.0, 409);
    assert!(r.1.contains("computer_spento"));
    b.p.stato.motore.computer().spegni_tutti().await;
    assert!(b.p.stato.motore.computer().posto(&id2).is_none());
}

#[tokio::test]
async fn processo_morto_errore_e_ricrea() {
    let b = banco(Some(("FINTO_MUORI_CHROME", "1.5"))).await;
    let id = b.assistente("Fragile").await;
    let base = format!("/api/v1/assistenti/{id}/computer");
    let (_, v) = b.chiama("POST", &format!("{base}/accendi")).await;
    assert_eq!(v["stato"], "acceso", "{v}");
    let mut v = Value::Null;
    for _ in 0..80 {
        v = b.chiama("GET", &base).await.1;
        if v["stato"] == "errore" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(v["stato"], "errore", "{v}");
    assert!(v["errore"].as_str().unwrap().contains("Chrome"), "{v}");
    assert!(b.p.stato.motore.computer().posto(&id).is_none());
    let (_, v) = b.chiama("POST", &format!("{base}/ricrea")).await;
    assert_eq!(v["stato"], "acceso", "{v}");
    assert_eq!(v["errore"], Value::Null);
    b.p.stato.motore.computer().spegni_tutti().await;
}

#[tokio::test]
async fn programmi_mancanti() {
    let b = banco(None).await;
    let id = b.assistente("Senza").await;
    let mut p = Programmi::finti(Path::new("/x.py"), Path::new("/tmp"));
    p.xvfb = None;
    b.p.stato.motore.computer().imposta_programmi(p);
    let (s, v) = b
        .chiama("POST", &format!("/api/v1/assistenti/{id}/computer/accendi"))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert!(v.to_string().contains("Xvfb"), "{v}");
}

#[tokio::test]
async fn terminale_websocket_verso_ttyd() {
    let b = banco(None).await;
    let id = b.assistente("Terminale").await;
    let base = format!("/api/v1/assistenti/{id}/computer");
    let (_, v) = b.chiama("POST", &format!("{base}/accendi")).await;
    assert_eq!(v["stato"], "acceso", "{v}");
    let (mut ws, risp) =
        tokio_tungstenite::connect_async(b.richiesta_ws(&format!("{base}/terminale/ws"), "tty"))
            .await
            .unwrap();
    assert_eq!(risp.headers().get("sec-websocket-protocol").unwrap(), "tty");
    ws.send(Message::text("ls")).await.unwrap();
    let m = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(m.into_text().unwrap().as_str(), "eco:ls");
    ws.send(Message::binary(b"1x".to_vec())).await.unwrap();
    let m = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(m.into_data().as_ref(), b"eco:1x");
    b.p.stato.motore.computer().spegni_tutti().await;
}

async fn reqwest_get(b: &Banco, percorso: &str) -> (u16, String) {
    let r = reqwest::Client::new()
        .get(format!("http://{}{percorso}", b.indirizzo))
        .header(header::COOKIE, &b.cookie)
        .send()
        .await
        .unwrap();
    (r.status().as_u16(), r.text().await.unwrap())
}
