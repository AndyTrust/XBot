//! Prove delle menzioni (Fase 2.D S2) con il provider finto, che risponde col testo rovesciato.

mod comune;
mod s2_comune;

use comune::*;
use s2_comune::*;

fn conta(p: &Prova, sql: &str, parametro: &str) -> i64 {
    p.db.con(|c| Ok(c.query_row(sql, [parametro], |r| r.get(0)).unwrap()))
        .unwrap()
}

async fn aspetta(cond: impl Fn() -> bool) {
    for _ in 0..500 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("condizione mai vera");
}

fn finto() -> Arc<dyn Provider> {
    Arc::new(Finto::con_ritardo(Duration::from_millis(2)))
}

#[tokio::test]
async fn menzione_dell_utente_arriva_incorniciata() {
    let p = prova_s2(vec![finto()], None).await;
    let c = p.entra().await;
    let (_a, sid_a) = assistente(&p, &c, "Aa", "finto").await;
    let (_cc, sid_c) = assistente(&p, &c, "Cc", "finto").await;
    let (s, v, _) = p.chiama("GET", "/api/v1/assistenti", None, Some(&c)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        v.as_array().unwrap().iter().any(|a| a["menzione"] == "cc"),
        "{v}"
    );

    p.invia(&c, &sid_a, &xbot_core::nuovo_id(), "ciao @cc e @nessuno")
        .await;
    aspetta(|| {
        conta(
            &p,
            "SELECT COUNT(*) FROM invio WHERE sessione_id = ?1",
            &sid_c,
        ) == 1
    })
    .await;
    let righe = p.righe(&c, &sid_c, 0).await;
    let eco = righe["righe"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["ruolo"] == "utente")
        .unwrap()
        .clone();
    assert_eq!(
        eco["corpo"]["testo"],
        "Mario, dalla chat con Aa, ti dice: ciao @cc e @nessuno"
    );
    assert_eq!(eco["corpo"]["origine"], format!("menzione:{sid_a}"));
    // Né la risposta di Aa né quella di Cc nominano qualcuno: niente catena.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        conta(
            &p,
            "SELECT COUNT(*) FROM invio WHERE origine LIKE ?1",
            "collega:%"
        ),
        0
    );
}

#[tokio::test]
async fn catena_fra_assistenti_fermata_al_quarto_passo() {
    let p = prova_s2(vec![finto()], None).await;
    let c = p.entra().await;
    let (_a, sid_a) = assistente(&p, &c, "Aa", "finto").await;
    let (bid, sid_b) = assistente(&p, &c, "Bb", "finto").await;
    // Aa risponde «@bb aa@»; ogni risposta rovesciata nomina l'altro: A→B→A→B, poi stop.
    p.invia(&c, &sid_a, &xbot_core::nuovo_id(), "@aa bb@").await;
    let avvisi = "SELECT COUNT(*) FROM riga WHERE sessione_id = ?1 AND genere = 'avviso'
                  AND json_extract(corpo_json, '$.codice') = 'catena_fermata'";
    aspetta(|| conta(&p, avvisi, &sid_b) == 1).await;
    assert_eq!(
        conta(
            &p,
            "SELECT COUNT(*) FROM invio WHERE origine LIKE ?1",
            "collega:%"
        ),
        3
    );
    let ruoli = "SELECT COUNT(*) FROM riga WHERE sessione_id = ?1 AND ruolo = 'collega'";
    assert_eq!(conta(&p, ruoli, &sid_b), 2);
    assert_eq!(conta(&p, ruoli, &sid_a), 1);
    let righe = p.righe(&c, &sid_b, 0).await;
    let avviso = righe["righe"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["genere"] == "avviso")
        .unwrap()
        .clone();
    assert!(
        avviso["corpo"]["testo"]
            .as_str()
            .unwrap()
            .starts_with("Catena fra assistenti fermata")
    );
    aspetta(|| {
        conta(
            &p,
            "SELECT in_attesa_utente FROM stato_lettura WHERE assistente_id = ?1",
            &bid,
        ) == 1
    })
    .await;
}
