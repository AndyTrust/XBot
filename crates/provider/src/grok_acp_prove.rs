use super::*;

/// Il campione vero: testo a pezzi, strumento con permesso, risposta con l'uso del turno.
#[test]
fn campione_vero() {
    let campione = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("prove/grok-acp-esempio.jsonl"),
    )
    .unwrap();
    let mut t = Traduttore::default();
    let mut ev = Vec::new();
    let mut permessi = 0;
    for l in campione.lines() {
        let v: Value = serde_json::from_str(l).unwrap();
        match v["method"].as_str() {
            Some("session/update") => ev.extend(t.aggiornamento(&v["params"]["update"])),
            Some("session/request_permission") => {
                permessi += 1;
                assert_eq!(nome_strumento(&v["params"]["toolCall"]), "Bash");
                assert_eq!(
                    input_strumento(&v["params"]["toolCall"])["command"],
                    "echo x > prova4.txt"
                );
            }
            None if v["id"] == 4 => ev.extend(t.fine(&v["result"], "S")),
            _ => {}
        }
    }
    assert_eq!(permessi, 1);
    let testo: String = ev
        .iter()
        .filter_map(|e| match e {
            EventoTurno::TestoDelta(d) => Some(d.as_str()),
            _ => None,
        })
        .collect();
    assert!(testo.starts_with("Eseguo solo quel comando."), "{testo}");
    assert!(ev.contains(&EventoTurno::ChiamataStrumento {
        nome: "run_terminal_command".into(),
        input_json: json!({"command": "echo x > prova4.txt", "description": "Scrive x in prova4.txt"}),
    }));
    assert!(
        ev.iter()
            .any(|e| matches!(e, EventoTurno::EsitoStrumento { ok: true, .. }))
    );
    let Some(EventoTurno::Finito {
        costo_usd: Some(c),
        token_uscita: Some(u),
        sessione_esterna_id: Some(s),
        ..
    }) = ev.last()
    else {
        panic!("{:?}", ev.last());
    };
    assert!(*c > 0.0 && *c < 1.0, "{c}");
    assert!(*u > 0);
    assert_eq!(s, "S");
}

#[test]
fn annullato_dopo_rifiuto_non_e_interruzione() {
    let r = json!({ "stopReason": "cancelled", "_meta": { "usage": {} } });
    let mut t = Traduttore::default();
    assert!(matches!(t.fine(&r, "S")[0], EventoTurno::Fallito { .. }));
    let mut t = Traduttore {
        negato: true,
        ..Default::default()
    };
    assert!(matches!(t.fine(&r, "S")[0], EventoTurno::Finito { .. }));
}
