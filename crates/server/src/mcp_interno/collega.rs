//! I collegamenti e i segreti dalla chat (2026-10-25): il bot prepara tutto, l'utente scrive
//! token e password solo nel campo protetto che si apre in chat, oppure apre il link del consenso.
//! Nessun valore torna al bot né resta nella riga della richiesta.
//! `collegamenti_catalogo`, `collegamento_aggiungi`, `cassaforte_salva`.

use serde_json::{Map, Value, json};
use xbot_core::{GenereCassaforte, Risposta, TipoRichiesta};
use xbot_db::cassaforte_voci::{self, DatiVoce};
use xbot_db::collegamenti::{self as repo, NuovoCollegamento};

use super::TurnoMcp;
use super::strumenti::Esito;
use crate::Stato;
use crate::rotte::catalogo;

fn da<T>(r: Risposta<T>) -> Result<T, String> {
    r.map_err(|e| e.testo)
}

fn arg<'a>(a: &'a Map<String, Value>, k: &str) -> &'a str {
    a.get(k).and_then(Value::as_str).map_or("", str::trim)
}

fn base(m: &Stato) -> String {
    m.servizi
        .url_pubblico
        .clone()
        .unwrap_or_else(|| "http://localhost".into())
        .trim_end_matches('/')
        .to_string()
}

/// Lo stesso servizio: stesso url (http) o stesso comando e argomenti (stdio).
fn stesso(voce: &Value, c: &Value) -> bool {
    match voce["trasporto"].as_str() {
        Some("http") => {
            let (u, v) = (voce["url"].as_str(), c["url"].as_str());
            u.is_some() && u == v
                || (voce["id"] == "google"
                    && v.is_some_and(|x| x.ends_with(catalogo::GOOGLE_PERCORSO)))
        }
        _ => {
            voce["comando"] == c["comando"]
                && voce["argomenti"] == c.get("argomenti").cloned().unwrap_or(Value::Null)
        }
    }
}

fn come(v: &Value) -> &'static str {
    let si = |k: &str| v[k].as_bool().unwrap_or(false);
    if si("client_nel_server") {
        "serve il client Google sul server (istruzioni)"
    } else if si("richiede_client") {
        "serve un'app dell'utente (Client ID e segreto, nel campo protetto)"
    } else if si("richiede_oauth") {
        "un clic: consenso con l'account"
    } else if v["campi"].as_array().is_some_and(|c| !c.is_empty()) {
        "chiede token o password nel campo protetto"
    } else {
        "pronto, nessun accesso"
    }
}

fn stato_di(c: &Value) -> String {
    let acceso = c["abilitato"].as_bool().unwrap_or(true);
    let oauth = c["oauth"]["stato"].as_str();
    let prova = c["ultima_prova_ok"].as_bool();
    match (acceso, oauth, prova) {
        (false, ..) => "collegato ma spento".into(),
        (_, Some("ok"), _) | (_, None, Some(true)) => "collegato e funzionante".into(),
        (_, Some(s), _) => format!("collegato, accesso {s}: va ricollegato"),
        (_, None, Some(false)) => "collegato, l'ultima prova è fallita".into(),
        _ => "collegato, mai provato".into(),
    }
}

/// `collegamenti_catalogo`: le voci del catalogo con come si collegano e se l'utente le ha già.
pub async fn catalogo_per_bot(m: &Stato, cerca: &str) -> Esito {
    let miei = da(m.db.esegui(repo::elenca).await)?;
    let cerca = cerca.to_lowercase();
    let v: Vec<Value> = catalogo::elenco(&base(m))
        .into_iter()
        .filter(|x| {
            cerca.is_empty()
                || format!("{} {} {}", x["id"], x["nome"], x["descrizione"])
                    .to_lowercase()
                    .contains(&cerca)
        })
        .map(|x| {
            let mio = miei.iter().find(|c| stesso(&x, c));
            json!({
                "voce": x["id"], "nome": x["nome"], "descrizione": x["descrizione"],
                "come_si_collega": come(&x),
                "stato": mio.map_or_else(|| "non collegato".to_string(), stato_di),
                "nota": x["nota"],
            })
        })
        .collect();
    let mut t = Value::Array(v).to_string();
    t.push_str(
        "\nPer collegare: `collegamento_aggiungi` con {voce}. Se un servizio non è nel catalogo, \
         `collegamento_aggiungi` con {nome, url} (server MCP remoto) o {nome, comando, argomenti}.",
    );
    Ok(t)
}

/// Apre il campo protetto in chat con questi campi e aspetta l'utente: i valori, mai nel testo.
async fn chiedi_campi(
    m: &Stato,
    t: &TurnoMcp,
    titolo: &str,
    aiuto: Option<String>,
    campi: Vec<Value>,
) -> Result<Map<String, Value>, String> {
    chiedi_campi_con(m, t, titolo, aiuto, campi, false).await
}

/// Come `chiedi_campi`; con `ambito_scegli` il box fa scegliere «solo questo assistente» o «tutti»
/// (torna in `valori.ambito`).
async fn chiedi_campi_con(
    m: &Stato,
    t: &TurnoMcp,
    titolo: &str,
    aiuto: Option<String>,
    campi: Vec<Value>,
    ambito_scegli: bool,
) -> Result<Map<String, Value>, String> {
    let carico = json!({
        "titolo": titolo, "campi": campi, "senza_salva": true, "aiuto": aiuto,
        "turno_id": t.turno_id, "ambito_scegli": ambito_scegli,
    });
    let (stato, _, valori) = da(super::permesso::attendi_valori(
        m,
        t,
        TipoRichiesta::Modulo,
        carico,
        m.motore.s2().attesa(),
    )
    .await)?;
    match stato.as_str() {
        "approvata" => {}
        "negata" => return Err("L'utente ha annullato.".into()),
        "ritirata" => return Err("Richiesta ritirata: il turno è stato interrotto.".into()),
        _ => return Err("L'utente non ha risposto in tempo: richiamalo quando è pronto.".into()),
    }
    let mut v = match valori {
        Value::Object(o) => o,
        _ => return Err("nessun valore arrivato: richiamalo".into()),
    };
    v.remove("salva");
    if v.values().any(|x| x == "***") {
        return Err("i valori non sono arrivati (risposta letta in ritardo): richiamalo".into());
    }
    Ok(v)
}

/// Il link del consenso per un collegamento OAuth (l'utente lo apre, il resto lo fa il server).
async fn link_consenso(m: &Stato, id: &str) -> Result<String, String> {
    let ritorno = format!("{}/api/v1/oauth/ritorno", base(m));
    da(crate::oauth::avvia(
        &m.db,
        m.servizi.cassaforte.clone(),
        &m.oauth_tentativi,
        id,
        &ritorno,
    )
    .await)
}

async fn prova(m: &Stato, id: &str) -> String {
    use axum::extract::{Path, State};
    match crate::rotte::collegamenti::prova(State(m.clone()), Path(id.to_string())).await {
        Ok(axum::Json(v)) if v["ok"] == true => {
            let n = v["strumenti"].as_array().map_or(0, Vec::len);
            format!("Prova riuscita: {n} strumenti.")
        }
        Ok(axum::Json(v)) => format!(
            "La prova non è riuscita: {}",
            v["errore"].as_str().unwrap_or("errore sconosciuto")
        ),
        Err(e) => format!("La prova non è partita: {e:?}"),
    }
}

/// `collegamento_aggiungi`: dal catalogo (`voce`) o personalizzato (`nome` + `url` o `comando`).
/// Crea il collegamento se manca, chiede nel campo protetto quello che serve, prepara il link del
/// consenso, prova. `ricollega=true` rifà accesso o token anche se c'è già.
pub async fn aggiungi(m: &Stato, t: &TurnoMcp, a: &Map<String, Value>) -> Esito {
    let k = m.servizi.cassaforte.clone();
    let id_voce = arg(a, "voce");
    let ricollega = a.get("ricollega").and_then(Value::as_bool).unwrap_or(false);
    let voce = if id_voce.is_empty() {
        let (nome, url, comando) = (arg(a, "nome"), arg(a, "url"), arg(a, "comando"));
        if nome.is_empty() || (url.is_empty() && comando.is_empty()) {
            return Err(
                "serve {voce} del catalogo, oppure {nome, url} o {nome, comando, argomenti}".into(),
            );
        }
        let campi: Vec<Value> = a
            .get("campi")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        json!({
            "id": Value::Null, "nome": nome,
            "trasporto": if url.is_empty() { "stdio" } else { "http" },
            "url": (!url.is_empty()).then_some(url),
            "comando": (!comando.is_empty()).then_some(comando),
            "argomenti": a.get("argomenti").cloned().unwrap_or(json!([])),
            "categoria": "personalizzati", "campi": campi,
            "richiede_oauth": false, "richiede_client": false, "client_nel_server": false,
        })
    } else {
        catalogo::elenco(&base(m))
            .into_iter()
            .find(|x| x["id"] == id_voce)
            .ok_or_else(|| {
                format!(
                    "«{id_voce}» non è nel catalogo: usa collegamenti_catalogo, o {{nome, url}}"
                )
            })?
    };
    let nome = voce["nome"].as_str().unwrap_or("collegamento").to_string();
    let miei = da(m.db.esegui(repo::elenca).await)?;
    let esistente = miei.iter().find(|c| stesso(&voce, c)).cloned();
    let nuovo = esistente.is_none();
    let id = match &esistente {
        Some(c) => c["id"].as_str().unwrap_or_default().to_string(),
        None => {
            let testi = |k: &str| voce[k].as_str().map(str::to_string);
            let n = NuovoCollegamento {
                nome: nome.clone(),
                trasporto: testi("trasporto").unwrap_or_else(|| "stdio".into()),
                comando: testi("comando"),
                argomenti: serde_json::from_value(voce["argomenti"].clone()).unwrap_or_default(),
                url: testi("url"),
                categoria: testi("categoria"),
                richiede_oauth: Some(voce["richiede_oauth"].as_bool().unwrap_or(false)),
                ..Default::default()
            };
            let k2 = k.clone();
            da(m.db.esegui(move |c| repo::crea(c, k2.as_deref(), &n)).await)?
        }
    };
    let mut fatto = vec![if nuovo {
        format!("«{nome}» aggiunto ai collegamenti.")
    } else {
        format!("«{nome}» c'era già.")
    }];

    // Token, password, indirizzi: nel campo protetto, poi cifrati nel collegamento.
    let campi = voce["campi"].as_array().cloned().unwrap_or_default();
    if !campi.is_empty() && (nuovo || ricollega) {
        let aiuto: Vec<String> = campi
            .iter()
            .filter_map(|c| {
                c["aiuto"]
                    .as_str()
                    .map(|a| format!("{}: {a}", c["etichetta"].as_str().unwrap_or("")))
            })
            .collect();
        let per_modulo: Vec<Value> = campi
            .iter()
            .map(|c| {
                json!({ "nome": c["nome"], "etichetta": c["etichetta"],
                        "tipo": if c["segreto"] == true { "segreto" } else { "testo" },
                        "obbligatorio": true })
            })
            .collect();
        let valori = chiedi_campi(
            m,
            t,
            &format!("Collega «{nome}»"),
            (!aiuto.is_empty()).then(|| aiuto.join("\n")),
            per_modulo,
        )
        .await?;
        let mut amb = Map::new();
        let mut intest = Map::new();
        let id_cat = voce["id"].as_str().unwrap_or_default();
        let def = catalogo::campi_di(id_cat);
        for (chiave, valore) in valori {
            let prefisso = def
                .iter()
                .find(|d| d.nome == chiave)
                .and_then(|d| d.intestazione);
            let testo = valore.as_str().unwrap_or_default().trim().to_string();
            match prefisso {
                Some(p) => intest.insert(chiave, Value::from(format!("{p}{testo}"))),
                None => amb.insert(chiave, Value::from(testo)),
            };
        }
        let mut cambi = Map::new();
        if !amb.is_empty() {
            cambi.insert("ambiente".into(), Value::Object(amb));
        }
        if !intest.is_empty() {
            cambi.insert("intestazioni".into(), Value::Object(intest));
        }
        let (k2, id2) = (k.clone(), id.clone());
        da(m.db
            .esegui(move |c| repo::aggiorna(c, k2.as_deref(), &id2, &cambi))
            .await)?;
        fatto.push(
            "Valori salvati cifrati nel collegamento (non li vedi e non servono a te).".into(),
        );
    }

    let riga = {
        let id2 = id.clone();
        da(m.db.esegui(move |c| repo::leggi(c, &id2)).await)?
    };
    if voce["client_nel_server"] == true {
        fatto.push(format!(
            "Google vuole il client sul server Google accanto a XBot: dai all'utente queste istruzioni.\n{}",
            voce["client_istruzioni"].as_str().unwrap_or("")
        ));
    } else if voce["richiede_client"] == true && (riga["client_registrato"] != true || ricollega) {
        let istruzioni = voce["client_istruzioni"].as_str().map(str::to_string);
        let v = chiedi_campi(
            m,
            t,
            &format!("App OAuth per «{nome}»"),
            istruzioni,
            vec![
                json!({ "nome": "client_id", "etichetta": "Client ID", "tipo": "testo", "obbligatorio": true }),
                json!({ "nome": "client_secret", "etichetta": "Client Secret", "tipo": "segreto", "obbligatorio": true }),
            ],
        )
        .await?;
        let cid = v["client_id"]
            .as_str()
            .unwrap_or_default()
            .trim()
            .to_string();
        let segreto = v["client_secret"]
            .as_str()
            .filter(|x| !x.trim().is_empty())
            .map(|x| xbot_core::SegretoTesto::from(x.trim().to_string()));
        let (k2, id2) = (k.clone(), id.clone());
        da(m.db
            .esegui(move |c| {
                xbot_db::oauth::imposta_client(c, k2.as_deref(), &id2, &cid, segreto.as_ref())
            })
            .await)?;
        fatto.push("App OAuth salvata.".into());
    }

    let oauth_ok = riga["oauth"]["stato"] == "ok";
    let vuole_oauth = riga["richiede_oauth"] == true && voce["client_nel_server"] != true;
    if vuole_oauth && (!oauth_ok || ricollega) {
        let url = link_consenso(m, &id).await?;
        fatto.push(format!(
            "Manca solo il consenso dell'utente. Scrivigli questo link così com'è, come link cliccabile \
             «Collega {nome}»: {url}\nApre la pagina di {nome}, l'utente accede e approva, poi torna in \
             XBot da solo. Il link vale pochi minuti e una volta. Gli strumenti di «{nome}» arrivano \
             dal messaggio dopo."
        ));
        return Ok(fatto.join("\n"));
    }
    fatto.push(prova(m, &id).await);
    fatto.push(format!(
        "Gli strumenti di «{nome}» arrivano dal prossimo messaggio dell'utente (questo turno è già partito)."
    ));
    if nome == "Microsoft 365" {
        fatto.push(
            "Al primo uso chiama lo strumento «login» di Microsoft 365: dà un codice e un indirizzo \
             (microsoft.com/devicelogin) da passare all'utente; poi «verify-login»."
                .into(),
        );
    }
    Ok(fatto.join("\n"))
}

fn variabile_ok(v: &str) -> bool {
    xbot_db::cassaforte_generi::variabile_valida(v)
        && !xbot_db::cassaforte_generi::variabile_vietata(v)
}

/// `cassaforte_salva`: l'utente scrive il valore nel campo protetto in chat e va dritto in
/// cassaforte. Al bot torna solo l'id della voce.
pub async fn cassaforte_salva(m: &Stato, t: &TurnoMcp, a: &Map<String, Value>) -> Esito {
    let k = m.servizi.chiave().map_err(|e| e.testo)?;
    let titolo = arg(a, "titolo");
    if titolo.is_empty() {
        return Err("serve il titolo della voce (es. «Gmail personale», «Token Hostinger»)".into());
    }
    let accesso = arg(a, "genere") == "accesso";
    let variabile = if accesso {
        String::new()
    } else {
        let v = arg(a, "variabile");
        if v.is_empty() {
            xbot_db::cassaforte_voci::nome_variabile(titolo, &Value::Null)
                .trim_start_matches("XBOT_CASSAFORTE_")
                .to_string()
        } else {
            v.to_uppercase()
        }
    };
    // Il nome si controlla PRIMA di aprire il box: un nome riservato (XBOT_*, PATH, CLAUDE_*…) o
    // non valido diventa SEGRETO_<nome>, così il valore che l'utente scrive non va perso.
    let variabile = if accesso || variabile_ok(&variabile) {
        variabile
    } else {
        let pulito: String = variabile
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_uppercase()
                } else {
                    '_'
                }
            })
            .collect();
        let altro = format!("SEGRETO_{}", pulito.trim_matches('_'));
        if variabile_ok(&altro) {
            altro
        } else {
            "SEGRETO".into()
        }
    };
    let sito = arg(a, "sito");
    let mut campi = Vec::new();
    if accesso {
        campi.push(
            json!({ "nome": "utente", "etichetta": "Utente o email", "tipo": "testo",
            "obbligatorio": false, "valore": arg(a, "utente") }),
        );
    }
    campi.push(json!({ "nome": "valore",
        "etichetta": if accesso { "Password" } else { "Valore" },
        "tipo": "segreto", "obbligatorio": true }));
    let motivo = arg(a, "motivo");
    let aiuto = (!motivo.is_empty()).then(|| motivo.to_string());
    let mut v = chiedi_campi_con(
        m,
        t,
        &format!("Salva in cassaforte «{titolo}»"),
        aiuto,
        campi,
        !accesso,
    )
    .await?;
    let per_tutti = v
        .remove("ambito")
        .and_then(|x| x.as_str().map(str::to_string))
        == Some("tutti".to_string());
    let valore = v
        .remove("valore")
        .and_then(|x| x.as_str().map(str::to_string))
        .unwrap_or_default();
    let utente = v
        .remove("utente")
        .and_then(|x| x.as_str().map(str::to_string))
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| arg(a, "utente").to_string());
    let (genere, rif, cam) = if accesso {
        let mut r = Map::new();
        if !utente.is_empty() {
            r.insert("utente".into(), Value::from(utente));
        }
        (
            GenereCassaforte::Accesso,
            r,
            json!({ "password": valore })
                .as_object()
                .cloned()
                .unwrap_or_default(),
        )
    } else {
        (
            GenereCassaforte::Segreto,
            json!({ "variabile": variabile, "descrizione": motivo })
                .as_object()
                .cloned()
                .unwrap_or_default(),
            json!({ "valore": valore })
                .as_object()
                .cloned()
                .unwrap_or_default(),
        )
    };
    // Un segreto arriva come variabile d'ambiente agli assistenti scelti, sul Mac e sulla VPS.
    let assistenti = if accesso {
        None
    } else if per_tutti {
        let tutti = da(m.db.esegui(|c| xbot_db::assistenti::elenca(c, false)).await)?;
        Some(
            tutti
                .iter()
                .filter_map(|x| x["id"].as_str().map(str::to_string))
                .collect(),
        )
    } else {
        Some(vec![t.assistente_id.clone()])
    };
    let d = DatiVoce {
        genere: Some(genere),
        titolo: Some(titolo.to_string()),
        siti: (!sito.is_empty()).then(|| vec![sito.to_string()]),
        riferimento: Some(rif),
        campi: Some(cam),
        assistenti,
        origine: Some("chat".into()),
    };
    let id = da(m.db.esegui(move |c| cassaforte_voci::crea(c, &k, &d)).await)?;
    tracing::info!(voce = %id, "cassaforte: voce salvata dalla chat");
    Ok(format!(
        "Salvato in cassaforte: voce {id} «{titolo}».{dove} Il valore non lo vedi. In questo turno, \
         se ti serve subito, chiama `cassaforte_chiedi` con {{id: \"{id}\", motivo}}.",
        dove = if accesso {
            String::new()
        } else {
            format!(
                " Dal prossimo messaggio arriva come variabile d'ambiente `{variabile}` {} \
                 (sul Mac e sulla VPS).",
                if per_tutti {
                    "a tutti gli assistenti"
                } else {
                    "a te"
                }
            )
        }
    ))
}

fn testo_o(a: &Map<String, Value>, k: &str, altrimenti: &str) -> String {
    Some(arg(a, k))
        .filter(|x| !x.is_empty())
        .unwrap_or(altrimenti)
        .to_string()
}

fn porta_o(a: &Map<String, Value>, k: &str, altrimenti: u16) -> u16 {
    a.get(k)
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        })
        .and_then(|p| u16::try_from(p).ok())
        .unwrap_or(altrimenti)
}

/// `posta_aggiungi_casella`: una casella nuova per la posta di XBot. L'utente scrive la password nel
/// campo protetto; va in cassaforte («Posta · <id>», genere accesso), i server in `posta.caselle`;
/// poi una lettura di prova.
pub async fn posta_aggiungi(m: &Stato, t: &TurnoMcp, a: &Map<String, Value>) -> Esito {
    let k = m.servizi.chiave().map_err(|e| e.testo)?;
    let indirizzo = arg(a, "indirizzo").to_lowercase();
    if !indirizzo.contains('@') {
        return Err("serve l'indirizzo della casella (es. info@azienda.it)".into());
    }
    let imap = arg(a, "imap_server");
    let smtp = arg(a, "smtp_server");
    if imap.is_empty() || smtp.is_empty() {
        return Err(
            "servono imap_server e smtp_server (es. imap.gmail.com e smtp.gmail.com; \
                    Aruba: imaps.aruba.it e smtps.aruba.it; PEC Aruba: imaps.pec.aruba.it e \
                    smtps.pec.aruba.it). Se non li sai, chiedili all'utente."
                .into(),
        );
    }
    let id: String = {
        let x = testo_o(a, "id", &indirizzo.replace('@', "-"));
        x.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect()
    };
    let ricollega = a.get("ricollega").and_then(Value::as_bool).unwrap_or(false);
    let attuali = da(m
        .db
        .esegui(|c| xbot_db::impostazioni::leggi(c, "posta.caselle"))
        .await)?
    .unwrap_or_else(|| json!({}));
    if attuali.get(&id).is_some() && !ricollega {
        return Err(format!(
            "la casella «{id}» c'è già: per cambiare la password richiamami con ricollega=true"
        ));
    }
    let gmail = imap.contains("gmail");
    let aiuto = if gmail {
        "Gmail vuole una «password per le app» (myaccount.google.com/apppasswords), non la password normale."
    } else {
        "La password della casella."
    };
    let campi = vec![
        json!({ "nome": "utente", "etichetta": "Utente (di solito l'indirizzo)", "tipo": "testo",
                "obbligatorio": true, "valore": testo_o(a, "utente", &indirizzo) }),
        json!({ "nome": "password", "etichetta": "Password", "tipo": "segreto", "obbligatorio": true }),
    ];
    let mut v = chiedi_campi(
        m,
        t,
        &format!("Collega la casella {indirizzo}"),
        Some(aiuto.to_string()),
        campi,
    )
    .await?;
    let utente = v
        .remove("utente")
        .and_then(|x| x.as_str().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| indirizzo.clone());
    let password = v
        .remove("password")
        .and_then(|x| x.as_str().map(str::to_string))
        .unwrap_or_default();
    let d = DatiVoce {
        genere: Some(GenereCassaforte::Accesso),
        titolo: Some(format!("Posta · {id}")),
        siti: Some(vec![imap.to_string()]),
        riferimento: json!({ "utente": utente }).as_object().cloned(),
        campi: json!({ "password": password }).as_object().cloned(),
        assistenti: None,
        origine: Some("chat".into()),
    };
    let k2 = k.clone();
    let voce = da(m
        .db
        .esegui(move |c| cassaforte_voci::crea(c, &k2, &d))
        .await)?;
    let mut tutte = attuali;
    tutte[id.as_str()] = json!({
        "indirizzo": indirizzo, "azienda": arg(a, "azienda"),
        "imap": [imap, porta_o(a, "imap_porta", 993)],
        "smtp": [smtp, porta_o(a, "smtp_porta", 465)],
        "voce": voce, "pec": a.get("pec").and_then(Value::as_bool).unwrap_or(false),
    });
    da(m.db
        .esegui(move |c| xbot_db::impostazioni::scrivi(c, "posta.caselle", &tutte))
        .await)?;
    tracing::info!(casella = %id, voce = %voce, "posta: casella aggiunta dalla chat");
    let ctx = crate::posta::Contesto {
        sessione_id: Some(t.sessione_id.clone()),
        assistente_id: Some(t.assistente_id.clone()),
        cartella: Some(t.cartella.clone()),
        attesa: None,
    };
    let mut prova = Map::new();
    prova.insert("casella".into(), Value::from(id.clone()));
    prova.insert("limite".into(), Value::from(1));
    let esito = crate::posta::chiama(m, &ctx, "posta_elenca", &prova, false).await;
    Ok(match esito {
        Some(Ok(_)) => format!(
            "Casella «{id}» collegata: password in cassaforte (voce {voce}), lettura di prova \
             riuscita. Da ora la usi con gli strumenti posta_* e casella «{id}»."
        ),
        Some(Err(e)) => format!(
            "Casella «{id}» salvata (password in cassaforte, voce {voce}), ma la lettura di prova \
             non è riuscita: {e}. Di solito è la password (Gmail vuole la password per le app) o \
             il server: correggi con ricollega=true."
        ),
        None => format!("Casella «{id}» salvata, prova non eseguita."),
    })
}
