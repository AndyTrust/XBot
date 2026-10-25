use super::*;

fn chiave() -> Chiave {
    Chiave::da_byte([7u8; 32])
}

fn nuovo(nome: &str) -> NuovoCollegamento {
    let mut amb = Map::new();
    amb.insert("TOKEN".into(), json!("segreto-1"));
    NuovoCollegamento {
        nome: nome.into(),
        trasporto: "stdio".into(),
        comando: Some("python3".into()),
        argomenti: vec!["srv.py".into()],
        ambiente: Some(amb),
        ..Default::default()
    }
}

#[test]
fn crea_legge_e_non_mostra_i_segreti() {
    let db = crate::db_prova();
    db.con(|c| {
        assert!(elenca(c)?.is_empty());
        assert_eq!(accesi(c)?, 0);
        let k = chiave();
        let id = crea(c, Some(&k), &nuovo("Prova"))?;
        let v = leggi(c, &id)?;
        assert_eq!(v["nome"], "Prova");
        assert_eq!(v["argomenti"][0], "srv.py");
        assert_eq!(v["intestazioni_presenti"], false);
        assert_eq!(v["ambiente_presente"], true);
        assert!(!v.to_string().contains("segreto-1"));
        let vivo = per_prova(c, Some(&k), &id)?;
        assert_eq!(vivo.ambiente[0].1.esponi(), "segreto-1");
        // Senza chiave i segreti non si leggono e non si scrivono.
        assert!(per_prova(c, None, &id).is_err());
        assert!(crea(c, None, &nuovo("B")).is_err());
        assert_eq!(accesi(c)?, 1);
        Ok(())
    })
    .unwrap();
}

#[test]
fn assistenti_permessi_e_strumenti() {
    let db = crate::db_prova();
    db.con(|c| {
        let k = chiave();
        let id = crea(c, Some(&k), &nuovo("Odoo"))?;
        c.execute(
            "INSERT INTO assistente (id, nome, creato_ms, aggiornato_ms) VALUES ('a1', 'A', 0, 0)",
            [],
        )
        .unwrap();
        // 2.L: acceso vale per tutti; la riga esclude.
        assert_eq!(per_assistente(c, Some(&k), "a1")?.len(), 1);
        imposta_esclusi(c, &id, &["a1".into()])?;
        assert!(per_assistente(c, Some(&k), "a1")?.is_empty());
        assert!(imposta_esclusi(c, &id, &["nessuno".into()]).is_err());
        assert_eq!(leggi(c, &id)?["esclusi"][0], "a1");
        imposta_esclusi(c, &id, &[])?;
        assert_eq!(per_assistente(c, Some(&k), "a1")?.len(), 1);
        let s = vec![
            StrumentoNoto {
                nome: "cerca".into(),
                descrizione: None,
                sola_lettura: true,
            },
            StrumentoNoto {
                nome: "scrivi".into(),
                descrizione: None,
                sola_lettura: false,
            },
        ];
        segna_prova(c, &id, Ok(&s))?;
        assert_eq!(permesso_per(c, &id, "cerca")?, "chiedi");
        let mut p = Map::new();
        p.insert("*lettura".into(), json!("sempre"));
        p.insert("scrivi".into(), json!("bloccato"));
        imposta_permessi(c, &id, &p)?;
        assert_eq!(permesso_per(c, &id, "cerca")?, "sempre");
        assert_eq!(permesso_per(c, &id, "scrivi")?, "bloccato");
        assert_eq!(permesso_per(c, &id, "altro")?, "chiedi");
        p.insert("x".into(), json!("forse"));
        assert!(imposta_permessi(c, &id, &p).is_err());
        let v = leggi(c, &id)?;
        assert_eq!(v["esclusi"], json!([]));
        assert_eq!(v["strumenti"][1]["permesso"], "bloccato");
        let mut m = Map::new();
        m.insert("abilitato".into(), json!(false));
        aggiorna(c, Some(&k), &id, &m)?;
        assert!(per_assistente(c, Some(&k), "a1")?.is_empty());
        m.insert("trasporto".into(), json!("http"));
        assert!(aggiorna(c, Some(&k), &id, &m).is_err(), "http senza url");
        elimina(c, &id)?;
        assert!(leggi(c, &id).is_err());
        Ok(())
    })
    .unwrap();
}

// --- 2.I ---
#[test]
fn patch_fonde_intestazioni_e_mostra_solo_i_nomi() {
    let db = crate::db_prova();
    db.con(|c| {
        let k = chiave();
        let id = crea(c, Some(&k), &nuovo("Prova"))?;
        let v = leggi(c, &id)?;
        assert_eq!(v["ambiente_nomi"], json!(["TOKEN"]));
        assert_eq!(v["intestazioni_nomi"], json!([]));
        let mut m = Map::new();
        m.insert("intestazioni".into(), json!({ "X-Uno": "a", "X-Due": "b" }));
        aggiorna(c, Some(&k), &id, &m)?;
        let mut m = Map::new();
        m.insert(
            "intestazioni".into(),
            json!({ "X-Uno": null, "X-Tre": "c" }),
        );
        m.insert("ambiente".into(), json!({ "ALTRO": "z" }));
        aggiorna(c, Some(&k), &id, &m)?;
        let v = leggi(c, &id)?;
        assert_eq!(v["intestazioni_nomi"], json!(["X-Due", "X-Tre"]));
        assert_eq!(v["ambiente_nomi"], json!(["ALTRO", "TOKEN"]));
        assert!(!v.to_string().contains("segreto-1"));
        let vivo = per_prova(c, Some(&k), &id)?;
        let amb: Vec<_> = vivo
            .ambiente
            .iter()
            .map(|(n, s)| (n.as_str(), s.esponi()))
            .collect();
        assert!(amb.contains(&("TOKEN", "segreto-1")) && amb.contains(&("ALTRO", "z")));
        let mut m = Map::new();
        m.insert("intestazioni".into(), Value::Null);
        aggiorna(c, Some(&k), &id, &m)?;
        assert_eq!(leggi(c, &id)?["intestazioni_nomi"], json!([]));
        Ok(())
    })
    .unwrap();
}

#[test]
fn oauth_entra_nel_turno_solo_se_ok() {
    let db = crate::db_prova();
    db.con(|c| {
        let k = chiave();
        let n = NuovoCollegamento {
            nome: "Granola".into(),
            trasporto: "http".into(),
            url: Some("https://mcp.granola.ai/mcp".into()),
            richiede_oauth: Some(true),
            intestazioni: Some({
                let mut m = Map::new();
                m.insert("authorization".into(), json!("Bearer a-mano"));
                m
            }),
            ..Default::default()
        };
        let id = crea(c, Some(&k), &n)?;
        c.execute(
            "INSERT INTO assistente (id, nome, creato_ms, aggiornato_ms) VALUES ('a1', 'A', 0, 0)",
            [],
        )
        .db()?;
        assert!(
            per_assistente(c, Some(&k), "a1")?.is_empty(),
            "senza accesso resta fuori"
        );
        assert_eq!(leggi(c, &id)?["richiede_oauth"], true);
        let a = crate::oauth::Accesso {
            collegamento_id: id.clone(),
            account: "utente@prova".into(),
            stato: "ok".into(),
            token: Some(crate::oauth::Token {
                access_token: "AT".into(),
                refresh_token: None,
                token_type: "bearer".into(),
                scope: None,
                expires_at_ms: None,
            }),
            client_id: None,
            client_secret: None,
            issuer: None,
            scopi: None,
            scade_ms: None,
        };
        crate::oauth::salva(c, Some(&k), &a)?;
        let v = per_assistente(c, Some(&k), "a1")?;
        assert_eq!(v.len(), 1);
        let auth: Vec<_> = v[0]
            .intestazioni
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case("authorization"))
            .map(|(_, s)| s.esponi().to_string())
            .collect();
        assert_eq!(auth, vec!["Bearer AT".to_string()], "l'OAuth vince");
        assert_eq!(leggi(c, &id)?["oauth"]["account"], "utente@prova");
        crate::oauth::segna(c, &id, "scaduto")?;
        assert!(per_assistente(c, Some(&k), "a1")?.is_empty());
        Ok(())
    })
    .unwrap();
}

/// 2.L: la migrazione 011 converte «acceso per questi» in «escluso per gli altri»; un
/// collegamento acceso per nessuno vale per tutti.
#[test]
fn migrazione_011_inverte_il_significato() {
    let db = crate::db_prova();
    db.con(|c| {
        let k = chiave();
        let a = crea(c, Some(&k), &nuovo("Acceso per a1"))?;
        let b = crea(c, Some(&k), &nuovo("Acceso per nessuno"))?;
        for (id, nome) in [("a1", "A"), ("a2", "B")] {
            c.execute(
                "INSERT INTO assistente (id, nome, creato_ms, aggiornato_ms) VALUES (?1, ?2, 0, 0)",
                [id, nome],
            )
            .db()?;
        }
        // Com'era prima della 011: una riga = acceso per quel bot.
        c.execute(
            "INSERT INTO collegamento_assistente (collegamento_id, assistente_id) VALUES (?1, 'a1')",
            [&a],
        )
        .db()?;
        let sql = crate::MIGRAZIONI
            .iter()
            .find(|(v, _)| *v == 11)
            .map(|(_, s)| *s)
            .unwrap_or_default();
        c.execute_batch(sql).db()?;
        assert_eq!(leggi(c, &a)?["esclusi"], json!(["a2"]));
        assert_eq!(leggi(c, &b)?["esclusi"], json!([]));
        let nomi = |aid: &str| -> Risposta<Vec<String>> {
            Ok(per_assistente(c, Some(&k), aid)?
                .into_iter()
                .map(|x| x.nome)
                .collect())
        };
        assert_eq!(nomi("a1")?, ["Acceso per a1", "Acceso per nessuno"]);
        assert_eq!(nomi("a2")?, ["Acceso per nessuno"]);
        Ok(())
    })
    .unwrap();
}
