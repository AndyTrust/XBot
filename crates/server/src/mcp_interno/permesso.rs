//! Lo strumento `permesso` (per `--permission-prompt-tool`): concessioni stabili, permessi dei
//! collegamenti, regole di revisione, altrimenti una `richiesta` all'utente e l'attesa (10 minuti).
//! Risponde nel formato di Claude Code: `{"behavior":"allow","updatedInput":{…}}` oppure
//! `{"behavior":"deny","message":"…"}` (verificato con Claude Code 2.1.310).

use serde_json::{Value, json};
use xbot_core::TipoRichiesta;
use xbot_db::{collegamenti, lettura, permessi, sessioni};

use super::TurnoMcp;
use crate::Stato;

/// Di che azione si tratta, per le concessioni stabili e per la richiesta.
#[derive(Debug, Clone, PartialEq)]
pub struct Azione {
    pub azione: String,
    pub destinatario: Option<String>,
    pub risorsa: Option<String>,
    pub tipo: TipoRichiesta,
    /// Il collegamento, se lo strumento è di un collegamento.
    pub collegamento: Option<(String, String)>,
}

pub fn consenti(input: &Value) -> Value {
    json!({ "behavior": "allow", "updatedInput": input })
}

pub fn nega(messaggio: impl Into<String>) -> Value {
    json!({ "behavior": "deny", "message": messaggio.into() })
}

fn testo(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}

/// Lo strumento di Claude Code tradotto in azione.
pub fn azione_di(t: &TurnoMcp, nome: &str, input: &Value) -> Azione {
    let fai = |azione: &str, destinatario: Option<String>, risorsa: Option<String>, tipo| Azione {
        azione: azione.into(),
        destinatario,
        risorsa,
        tipo,
        collegamento: None,
    };
    if let Some(resto) = nome.strip_prefix("mcp__")
        && let Some((server, strumento)) = resto.split_once("__")
        && let Some((_, id)) = t.collegamenti.iter().find(|(n, _)| n == server)
    {
        let mut a = fai(
            "strumento_mcp",
            Some(server.into()),
            Some(strumento.into()),
            TipoRichiesta::Collegamento,
        );
        a.collegamento = Some((id.clone(), strumento.into()));
        return a;
    }
    // Il destinatario è lo strumento stesso: così le concessioni «sempre» scritte da
    // `richieste::risolvi` (destinatario = strumento del carico) si ritrovano.
    let d = Some(nome.to_string());
    match nome {
        "Bash" => fai(
            "run-command",
            d,
            testo(input, "command"),
            TipoRichiesta::RevisioneComando,
        ),
        "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => fai(
            "write-file",
            d,
            testo(input, "file_path").or_else(|| testo(input, "notebook_path")),
            TipoRichiesta::PermessoLocale,
        ),
        "Read" | "Glob" | "Grep" => fai(
            "read-file",
            d,
            testo(input, "file_path").or_else(|| testo(input, "path")),
            TipoRichiesta::PermessoLocale,
        ),
        _ => fai(
            "use-app",
            Some(nome.into()),
            None,
            TipoRichiesta::PermessoLocale,
        ),
    }
}

/// Le regole di revisione: confronto testuale semplice (minuscole, contenuto) sul nome, sul
/// comando, sulla descrizione e sull'input. `blocca` vince su `consenti`.
pub fn da_regole(
    regole: &[Value],
    assistente_id: &str,
    nome: &str,
    input: &Value,
) -> Option<(bool, String)> {
    let mut dove = format!("{nome} {}", input).to_lowercase();
    for k in ["command", "description"] {
        if let Some(t) = input.get(k).and_then(Value::as_str) {
            dove.push(' ');
            dove.push_str(&t.to_lowercase());
        }
    }
    let attive = regole.iter().filter(|r| {
        r["attiva"].as_i64().unwrap_or(1) == 1
            && r["assistente_id"]
                .as_str()
                .is_none_or(|x| x == assistente_id)
    });
    let mut consentita = None;
    for r in attive {
        let t = r["testo"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        if t.is_empty() || !dove.contains(&t) {
            continue;
        }
        match r["tipo"].as_str() {
            Some("blocca") => return Some((false, t)),
            Some("consenti") => consentita = Some((true, t)),
            _ => {}
        }
    }
    consentita
}

/// Accorcia i testi lunghi dell'input per il carico della richiesta (mai tutto un file).
fn accorcia(v: &Value) -> Value {
    match v {
        Value::String(s) if s.chars().count() > 600 => Value::String(format!(
            "{}… ({} caratteri)",
            s.chars().take(600).collect::<String>(),
            s.chars().count()
        )),
        Value::Object(o) => {
            Value::Object(o.iter().map(|(k, v)| (k.clone(), accorcia(v))).collect())
        }
        Value::Array(a) => Value::Array(a.iter().take(50).map(accorcia).collect()),
        altro => altro.clone(),
    }
}

/// Decide su uno strumento. Non fallisce mai: un errore interno diventa `deny` con il motivo.
pub async fn decidi(m: &Stato, t: &TurnoMcp, nome: &str, input: &Value) -> Value {
    if nome.starts_with("mcp__xbot__") {
        return consenti(input);
    }
    if let Some(v) = senza_chiedere(m, t, nome, input).await {
        return v;
    }
    let a = azione_di(t, nome, input);
    chiedi_all_utente(m, t, nome, input, &a).await
}

/// Quello che si decide senza l'utente: permesso dello strumento del collegamento, concessioni
/// stabili, regole di revisione. `None`: va chiesto. Lo usa anche il canale delle macchine per
/// le richieste del Mac (2.L-bis), prima di mostrarle all'utente.
pub async fn senza_chiedere(m: &Stato, t: &TurnoMcp, nome: &str, input: &Value) -> Option<Value> {
    let a = azione_di(t, nome, input);
    let (aid, a2) = (t.assistente_id.clone(), a.clone());
    let (aid2, sid) = (aid.clone(), t.sessione_id.clone());
    let quadro =
        m.db.esegui(move |c| {
            let coll = match &a2.collegamento {
                Some((id, s)) => Some(collegamenti::permesso_per(c, id, s)?),
                None => None,
            };
            // Le concessioni stabili: `permessi::cerca` di S1 (la più specifica vince).
            let macchina = sessioni::leggi(c, &sid)?.macchina_id;
            let concessa = permessi::cerca(
                c,
                &a2.azione,
                a2.destinatario.as_deref(),
                a2.risorsa.as_deref(),
                Some(&aid2),
                macchina.as_deref(),
            )?;
            Ok((coll, concessa, permessi::regole(c)?))
        })
        .await;
    let (coll, concessa, regole) = match quadro {
        Ok(q) => q,
        Err(e) => return Some(nega(format!("permessi non leggibili: {}", e.testo))),
    };
    match coll.as_deref() {
        Some("sempre") => return Some(consenti(input)),
        Some("bloccato") => {
            return Some(nega(
                "Strumento bloccato dall'utente per questo collegamento.",
            ));
        }
        _ => {}
    }
    match concessa.as_deref() {
        Some("sempre") => return Some(consenti(input)),
        Some("mai") => return Some(nega("L'utente ha negato per sempre questa azione.")),
        _ => {}
    }
    da_regole(&regole, &aid, nome, input).map(|(ok, regola)| {
        if ok {
            consenti(input)
        } else {
            nega(format!("Bloccato dalla regola di revisione: «{regola}»."))
        }
    })
}

async fn chiedi_all_utente(
    m: &Stato,
    t: &TurnoMcp,
    nome: &str,
    input: &Value,
    a: &Azione,
) -> Value {
    let carico = json!({
        "azione": a.azione, "destinatario": a.destinatario, "risorsa": a.risorsa,
        "strumento": nome, "comando": input.get("command"), "descrizione": input.get("description"),
        "input": accorcia(input), "turno_id": t.turno_id,
    });
    let attesa = m.motore.s2().attesa();
    match attendi(m, t, a.tipo, carico, attesa).await {
        Ok((esito, risoluzione)) if esito == "approvata" => {
            let mut nuovo = input.clone();
            if nome == "Bash"
                && let Some(c) = risoluzione.get("comando_approvato").and_then(Value::as_str)
                && !c.trim().is_empty()
            {
                nuovo["command"] = Value::from(c);
            }
            consenti(&nuovo)
        }
        Ok((esito, _)) if esito == "ritirata" => {
            nega("Richiesta ritirata: il turno è stato interrotto.")
        }
        Ok((esito, risoluzione)) if esito == "negata" => {
            let motivo = risoluzione
                .get("motivo")
                .and_then(Value::as_str)
                .unwrap_or_default();
            nega(if motivo.is_empty() {
                "L'utente ha negato il permesso.".to_string()
            } else {
                format!("L'utente ha negato il permesso: {motivo}")
            })
        }
        Ok(_) => nega("L'utente non ha risposto in tempo."),
        Err(e) => nega(format!("richiesta non creata: {}", e.testo)),
    }
}

/// Crea la richiesta (con la sua riga in chat, `richieste::crea` di S1), segna «tocca a te» e
/// aspetta la risposta dell'utente (`richieste::attendi`: la sveglia `POST /richieste/{id}/risolvi`).
/// Alla scadenza la richiesta diventa `scaduta`. Restituisce (stato finale, risoluzione).
pub async fn attendi(
    m: &Stato,
    t: &TurnoMcp,
    tipo: TipoRichiesta,
    carico: Value,
    attesa: std::time::Duration,
) -> xbot_core::Risposta<(String, Value)> {
    let r = crate::richieste::crea(m, &t.sessione_id, tipo, carico, Some(attesa)).await?;
    let id = r["id"].as_str().unwrap_or_default().to_string();
    let aid = t.assistente_id.clone();
    let segna = |attende: bool| {
        let aid = aid.clone();
        async move {
            if let Err(e) =
                m.db.esegui(move |c| lettura::imposta_lavoro(c, &aid, true, attende))
                    .await
            {
                tracing::warn!(errore = %e, "stato di lettura non aggiornato");
            }
            m.bus.pubblica(&t.sessione_id, crate::bus::Segnale::Barra);
        }
    };
    segna(true).await;
    let esito = crate::richieste::attendi(m, &id, attesa).await;
    segna(false).await;
    let r = esito?;
    Ok((
        r.esito.come_testo().to_string(),
        json!({ "comando_approvato": r.comando_approvato, "richiesta_id": id, "valori": r.valori }),
    ))
}

/// Come `attendi`, ma restituisce anche i valori scritti dall'utente (modulo, segreto): stato,
/// id della richiesta, valori. I valori non vanno mai nei log né nel testo per l'assistente.
pub async fn attendi_valori(
    m: &Stato,
    t: &TurnoMcp,
    tipo: TipoRichiesta,
    carico: Value,
    attesa: std::time::Duration,
) -> xbot_core::Risposta<(String, String, Value)> {
    let (stato, mut r) = attendi(m, t, tipo, carico, attesa).await?;
    let id = r["richiesta_id"].as_str().unwrap_or_default().to_string();
    Ok((stato, id, r["valori"].take()))
}

#[cfg(test)]
mod test {
    use super::*;

    fn turno() -> TurnoMcp {
        TurnoMcp {
            turno_id: "t".into(),
            sessione_id: "s".into(),
            assistente_id: "a".into(),
            invio_id: "i".into(),
            gettone: "g".into(),
            passo: 0,
            cartella: "/tmp".into(),
            collegamenti: vec![("odoo".into(), "C1".into())],
        }
    }

    #[test]
    fn azioni() {
        let t = turno();
        let a = azione_di(&t, "Bash", &json!({"command": "ls -la"}));
        assert_eq!(
            (a.azione.as_str(), a.risorsa.as_deref()),
            ("run-command", Some("ls -la"))
        );
        assert_eq!(a.tipo, TipoRichiesta::RevisioneComando);
        let a = azione_di(&t, "mcp__odoo__cerca", &json!({}));
        assert_eq!(a.collegamento, Some(("C1".into(), "cerca".into())));
        let a = azione_di(&t, "mcp__altro__x", &json!({}));
        assert_eq!(a.azione, "use-app");
        assert_eq!(
            azione_di(&t, "Write", &json!({"file_path": "/a"})).azione,
            "write-file"
        );
    }

    #[test]
    fn regole_di_revisione() {
        let r = vec![
            json!({"tipo": "consenti", "testo": "git status", "attiva": 1, "assistente_id": null}),
            json!({"tipo": "blocca", "testo": "rm -rf", "attiva": 1, "assistente_id": null}),
            json!({"tipo": "blocca", "testo": "status", "attiva": 0, "assistente_id": null}),
        ];
        assert_eq!(
            da_regole(&r, "a", "Bash", &json!({"command": "git status"})).map(|x| x.0),
            Some(true)
        );
        assert_eq!(
            da_regole(&r, "a", "Bash", &json!({"command": "RM -RF /"})).map(|x| x.0),
            Some(false)
        );
        assert_eq!(da_regole(&r, "a", "Bash", &json!({"command": "ls"})), None);
    }

    #[test]
    fn formato_della_risposta() {
        let i = json!({"command": "ls"});
        assert_eq!(
            consenti(&i),
            json!({"behavior": "allow", "updatedInput": {"command": "ls"}})
        );
        assert_eq!(nega("no")["behavior"], "deny");
        let lungo = json!({"content": "x".repeat(2000)});
        assert!(
            accorcia(&lungo)["content"]
                .as_str()
                .unwrap()
                .contains("2000 caratteri")
        );
    }
}
