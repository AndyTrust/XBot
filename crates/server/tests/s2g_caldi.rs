//! Prove della Fase 2.G: grok in ACP con il permesso chiesto all'utente attraverso il socket.

mod comune;
mod s2_comune;

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use comune::*;
use s2_comune::*;
use xbot_provider::{Grok, Provider};

fn grok_finto() -> Arc<dyn Provider> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../provider/prove/grok-finto");
    let mut perm = std::fs::metadata(&p).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(&p, perm).unwrap();
    let sessioni = std::env::temp_dir().join(format!("xbot-2g-{}", xbot_core::nuovo_id()));
    Arc::new(
        Grok::nuovo(p)
            .con_casa(sessioni.join("grok-home"))
            .con_cartella_sessioni(sessioni),
    )
}

#[tokio::test]
async fn permesso_di_grok_passa_da_utente() {
    let p = prova_s2(vec![grok_finto()], None).await;
    let c = p.entra().await;
    let (_aid, sid) = assistente(&p, &c, "Grok", "grok").await;
    let e = p.invia(&c, &sid, &xbot_core::nuovo_id(), "[negato]").await;
    let tid = e["turno_id"].as_str().unwrap().to_string();
    let r = richiesta_aperta(&p, &c).await;
    assert_eq!(r["tipo"], "revisione_comando", "{r}");
    risolvi(&p, &c, r["id"].as_str().unwrap(), "approvata").await;
    assert_eq!(p.aspetta_turno(&c, &tid).await, "riuscito");
    assert_eq!(ultima_risposta(&p, &c, &sid).await, "]otagen[");
    // Il secondo turno va sullo stesso processo caldo.
    let (stato, testo) = turno(&p, &c, &sid, "ancora [pid]").await;
    assert_eq!(stato, "riuscito");
    assert!(testo.contains("turno 2"), "{testo}");
}
