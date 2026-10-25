//! Il canale MCP (Fase 2.D S2): `POST /mcp` (e `GET`/`DELETE`, come vuole il trasporto)
//! Streamable HTTP con `rmcp`, fuori da `/api/v1` e senza cookie. Ogni richiesta porta
//! `Authorization: Bearer <token>`; il token (SHA-256 in tabella `canale`) dice quale canale
//! è e quali strumenti può usare. Ogni chiamata va in `canale_chiamata`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Response};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData, RoleServer, ServerHandler};
use serde_json::{Map, Value, json};
use xbot_core::nuovo_id;
use xbot_db::canali::{self, CanaleAttivo};
use xbot_db::{provider, turni};

use crate::Stato;
use crate::mcp_interno::strumenti::{self, Esito};
use crate::mcp_interno::{strumento, testo};
use crate::motore::DatiInvio;

/// L'attesa massima di `invia_a_assistente`.
pub const ATTESA_MASSIMA_S: u64 = 120;
/// Il prefisso dell'errore del tetto orario (`429 troppe_chiamate`).
pub const TROPPE: &str = "429 troppe_chiamate";

/// Il router di `/mcp`, con l'autenticazione Bearer davanti.
pub fn router(stato: Stato) -> Router {
    let s2 = stato.clone();
    let servizio = StreamableHttpService::new(
        move || Ok(ServerCanale { stato: s2.clone() }),
        Arc::new(NeverSessionManager::default()),
        StreamableHttpServerConfig::default()
            .with_legacy_session_mode(false)
            .with_json_response(true)
            // Dietro Caddy l'host è il dominio pubblico: l'accesso lo decide il token.
            .disable_allowed_hosts(),
    );
    Router::new()
        .route_service("/mcp", servizio)
        .layer(from_fn_with_state(stato, autentica))
}

fn non_autorizzato(testo: &str) -> Response {
    let corpo = json!({ "errore": { "codice": "non_autorizzato", "testo": testo } });
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Bearer")],
        axum::Json(corpo),
    )
        .into_response()
}

/// Un errore di `rmcp` in testo semplice (es. 415 su JSON rotto) diventa `{"errore":{…}}`.
async fn in_formato(r: Response) -> Response {
    let stato = r.status();
    let json = r
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| t.contains("json") || t.contains("event-stream"));
    if !stato.is_client_error() || json {
        return r;
    }
    let corpo = axum::body::to_bytes(r.into_body(), 4096)
        .await
        .unwrap_or_default();
    let testo: String = String::from_utf8_lossy(&corpo).chars().take(200).collect();
    let codice = if stato == StatusCode::PAYLOAD_TOO_LARGE {
        "corpo_troppo_grande"
    } else {
        "richiesta_non_valida"
    };
    let v = json!({ "errore": { "codice": codice, "testo": testo.trim() } });
    (stato, axum::Json(v)).into_response()
}

/// Riconosce il canale dal token e lo mette nelle estensioni della richiesta.
async fn autentica(State(s): State<Stato>, mut req: Request, next: Next) -> Response {
    let token = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.strip_prefix("Bearer ")
                .or_else(|| v.strip_prefix("bearer "))
        })
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    let Some(token) = token else {
        return non_autorizzato("manca Authorization: Bearer <token>");
    };
    let h = crate::rotte::canali::hash_token(&token);
    match s.db.esegui(move |c| canali::per_hash(c, &h)).await {
        Ok(Some(canale)) => {
            req.extensions_mut().insert(canale);
            in_formato(next.run(req).await).await
        }
        Ok(None) => non_autorizzato("token non valido o revocato"),
        Err(e) => {
            tracing::error!(errore = %e, "canale non verificato");
            (StatusCode::INTERNAL_SERVER_ERROR, "errore interno").into_response()
        }
    }
}

fn definizioni() -> Vec<Tool> {
    let s = |t: &str| json!({ "type": "string", "description": t });
    let mut v = vec![
        strumento(
            "leggi_memoria",
            "Legge la memoria dell'utente: senza percorso la porta d'ingresso (Memoria.md), altrimenti il file indicato dentro la memoria.",
            json!({ "percorso": s("percorso relativo alla cartella della memoria") }),
            &[],
        ),
        strumento(
            "cerca_memoria",
            "Cerca parole nella memoria dell'utente.",
            json!({ "parole": s("parole da cercare") }),
            &["parole"],
        ),
        strumento(
            "scrivi_quaderno",
            "Scrive nel quaderno di un agente.",
            json!({ "agente": s("nome dell'agente"), "testo": s("cosa salvare") }),
            &["agente", "testo"],
        ),
        strumento(
            "scrivi_memoria",
            "Scrive nella memoria comune dell'utente (la cartella della memoria impostata in XBot): stato di avanzamento, decisioni, note di progetto. Solo file .md dentro la memoria. modo «aggiungi» (predefinito) mette in coda una sezione datata con l'autore; «sostituisci» riscrive il file.",
            json!({ "percorso": s("file .md relativo alla cartella Memoria, es. «Progetti/Esempio/Stato.md»"), "testo": s("cosa scrivere (Markdown)"), "modo": s("aggiungi | sostituisci"), "autore": s("chi scrive, es. «Reason · PrivateJet»") }),
            &["percorso", "testo"],
        ),
        strumento(
            "elenco_assistenti",
            "Gli assistenti di XBot con la @menzione e cosa fanno.",
            json!({}),
            &[],
        ),
        strumento(
            "invia_a_assistente",
            "Manda un messaggio a un assistente di XBot e aspetta la sua risposta (al massimo 120 secondi).",
            json!({ "menzione": s("@nome dell'assistente (se manca, quello del canale)"), "testo": s("il messaggio"), "attendi_secondi": { "type": "integer", "minimum": 0, "maximum": ATTESA_MASSIMA_S } }),
            &["testo"],
        ),
        strumento(
            "stato_quote",
            "Le quote d'uso dei provider (percentuale e azzeramento).",
            json!({}),
            &[],
        ),
        strumento(
            "cassaforte_elenco",
            "I titoli delle voci della cassaforte (mai i valori).",
            json!({}),
            &[],
        ),
        // --- routine da fuori (2026-10-25, l'utente: n8n-xbot tiene l'ora, XBot la routine) ---
        strumento(
            "esegui_routine",
            "Fa partire subito una routine di XBot (per nome o id) nella sessione del suo assistente, come farebbe il suo orologio. Risponde con esecuzione_id, sessione_id e invio_id: con invio_id si chiede l'esito a «esito_routine».",
            json!({ "routine": s("nome (anche parziale, senza distinzione di maiuscole) o id della routine") }),
            &["routine"],
        ),
        strumento(
            "esito_routine",
            "L'esito di una routine fatta partire con «esegui_routine»: aspetta la fine del turno (al massimo 120 secondi per chiamata, richiamabile) e dà lo stato e il testo della risposta dell'assistente.",
            json!({ "invio_id": s("invio_id dato da esegui_routine"), "attendi_secondi": { "type": "integer", "minimum": 0, "maximum": ATTESA_MASSIMA_S } }),
            &["invio_id"],
        ),
    ];
    v.extend(crate::posta::definizioni());
    v
}

/// Il server MCP del canale (uno per richiesta: il trasporto è senza sessione).
#[derive(Clone)]
pub struct ServerCanale {
    stato: Stato,
}

fn canale_di(c: &RequestContext<RoleServer>) -> Option<CanaleAttivo> {
    c.extensions
        .get::<http::request::Parts>()
        .and_then(|p| p.extensions.get::<CanaleAttivo>())
        .cloned()
}

/// Maschera i token dentro un testo: `plc_<…>` → `plc_…`, `Bearer <…>` → `Bearer …`.
pub fn maschera_testo(t: &str) -> String {
    let token =
        |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '~' | '+' | '/' | '=');
    let mut fuori = String::with_capacity(t.len());
    let mut resto = t;
    loop {
        let p = resto.find("plc_");
        let b = resto.find("Bearer ");
        let (pos, testa) = match (p, b) {
            (Some(p), Some(b)) if b < p => (b, "Bearer "),
            (Some(p), _) => (p, "plc_"),
            (None, Some(b)) => (b, "Bearer "),
            (None, None) => {
                fuori.push_str(resto);
                return fuori;
            }
        };
        fuori.push_str(&resto[..pos + testa.len()]);
        let dopo = &resto[pos + testa.len()..];
        let fine = dopo.find(|c: char| !token(c)).unwrap_or(dopo.len());
        if fine > 0 {
            fuori.push('…');
        }
        resto = &dopo[fine..];
    }
}

fn privato(k: &str, v: &Value) -> Value {
    let sensibile = [
        "password",
        "token",
        "segreto",
        "chiave",
        "valore",
        "authorization",
    ]
    .iter()
    .any(|x| k.to_lowercase().contains(x));
    match v {
        _ if sensibile => Value::from("***"),
        Value::String(t) => {
            let t = maschera_testo(t);
            if t.chars().count() > 200 {
                Value::from(format!("{}…", t.chars().take(200).collect::<String>()))
            } else {
                Value::from(t)
            }
        }
        Value::Object(o) => {
            Value::Object(o.iter().map(|(k, v)| (k.clone(), privato(k, v))).collect())
        }
        Value::Array(a) => Value::Array(a.iter().map(|v| privato(k, v)).collect()),
        altro => altro.clone(),
    }
}

/// Gli argomenti per il registro: chiavi sensibili nascoste, token mascherati, testi accorciati.
fn privati(a: &Map<String, Value>) -> Value {
    Value::Object(a.iter().map(|(k, v)| (k.clone(), privato(k, v))).collect())
}

fn arg<'a>(a: &'a Map<String, Value>, k: &str) -> &'a str {
    a.get(k).and_then(Value::as_str).unwrap_or_default()
}

/// `invia_a_assistente`: invio `origine = canale:<id>` nella sessione corrente dell'assistente,
/// poi attesa della fine del turno e testo della risposta.
pub async fn invia_a_assistente(
    s: &Stato,
    canale: &CanaleAttivo,
    menzione: &str,
    testo: &str,
    attendi_s: u64,
) -> Esito {
    if testo.trim().is_empty() {
        return Err("il testo è vuoto".into());
    }
    // Il token del canale è il consenso dell'utente: niente richiesta, ma un tetto orario.
    let kid = canale.id.clone();
    let fatti =
        s.db.esegui(move |c| canali::invii_ultima_ora(c, &kid))
            .await
            .map_err(|e| e.testo)?;
    if fatti >= canale.limite_orario {
        return Err(format!(
            "{TROPPE}: già {fatti} invii nell'ultima ora (limite {} per questo canale)",
            canale.limite_orario
        ));
    }
    let menzione = menzione.trim().trim_start_matches('@').to_lowercase();
    let predefinito = canale.assistente_id.clone();
    let assistente_id =
        s.db.esegui(move |c| {
            Ok(if menzione.is_empty() {
                predefinito
            } else {
                xbot_db::assistenti::trova_per_menzione(c, &menzione)?
            })
        })
        .await
        .map_err(|e| e.testo)?
        .ok_or("nessun assistente con quella menzione (e il canale non ne ha uno predefinito)")?;
    let aid = assistente_id.clone();
    let sessione =
        s.db.esegui(move |c| crate::menzioni::sessione_corrente(c, &aid))
            .await
            .map_err(|e| e.testo)?;
    let d = DatiInvio {
        id: nuovo_id(),
        testo: format!("Il canale «{}» ti scrive: {testo}", canale.nome),
        allegati: Value::Array(vec![]),
        risposta_a: None,
        provider: None,
        macchina: None,
        origine: format!("canale:{}", canale.id),
        composto_ms: None,
    };
    let e = s.motore.invia(&sessione, d).await.map_err(|e| e.testo)?;
    if e.stato == "rifiutato" || e.stato == "fallito" {
        return Err(format!(
            "invio {}: {}",
            e.stato,
            e.rifiuto.map(|r| r.to_string()).unwrap_or_default()
        ));
    }
    let fine = Instant::now() + Duration::from_secs(attendi_s.min(ATTESA_MASSIMA_S));
    loop {
        let (iid, sid) = (e.invio_id.clone(), sessione.clone());
        let t =
            s.db.esegui(move |c| {
                Ok(match turni::per_invio(c, &iid)? {
                    Some(t) if t.stato != "in_corso" => {
                        let testo = crate::motore::testo_del_turno(c, &sid, &t.id)?;
                        Some((t.stato, testo))
                    }
                    _ => None,
                })
            })
            .await
            .map_err(|e| e.testo)?;
        if let Some((stato, risposta)) = t {
            return Ok(json!({ "stato": stato, "risposta": risposta, "sessione_id": sessione, "invio_id": e.invio_id }).to_string());
        }
        if Instant::now() >= fine {
            return Ok(json!({ "stato": "in_corso", "risposta": null, "sessione_id": sessione, "invio_id": e.invio_id }).to_string());
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

/// `esegui_routine`: trova la routine (id, nome intero o pezzo del nome, senza maiuscole) e la fa
/// partire con l'orologio, senza spostare la scadenza del suo cron.
pub async fn esegui_routine(s: &Stato, canale: &CanaleAttivo, cercata: &str) -> Esito {
    let cercata = cercata.trim().to_string();
    if cercata.is_empty() {
        return Err("serve «routine»: nome o id".into());
    }
    let c2 = cercata.clone();
    let trovate =
        s.db.esegui(move |c| {
            let tutte = xbot_db::routine::elenca(c, None)?;
            let basso = c2.to_lowercase();
            let esatte: Vec<Value> = tutte
                .iter()
                .filter(|r| {
                    r["id"].as_str() == Some(c2.as_str())
                        || r["nome"].as_str().map(str::to_lowercase) == Some(basso.clone())
                })
                .cloned()
                .collect();
            Ok(if esatte.is_empty() {
                tutte
                    .into_iter()
                    .filter(|r| {
                        r["nome"]
                            .as_str()
                            .is_some_and(|n| n.to_lowercase().contains(&basso))
                    })
                    .collect()
            } else {
                esatte
            })
        })
        .await
        .map_err(|e| e.testo)?;
    let r = match trovate.as_slice() {
        [] => return Err(format!("nessuna routine che si chiami «{cercata}»")),
        [una] => una.clone(),
        tante => {
            let nomi: Vec<&str> = tante.iter().filter_map(|r| r["nome"].as_str()).collect();
            return Err(format!("«{cercata}» è ambiguo: {}", nomi.join(", ")));
        }
    };
    if r["abilitata"].as_i64() == Some(0) || r["abilitata"] == json!(false) {
        return Err(format!(
            "la routine «{}» è disabilitata",
            r["nome"].as_str().unwrap_or("?")
        ));
    }
    let v = crate::orologio::esegui(s, &r, false)
        .await
        .map_err(|e| e.testo)?;
    tracing::info!(canale = %canale.nome, routine = %r["nome"], "routine avviata dal canale");
    Ok(json!({
        "routine_id": r["id"], "routine": r["nome"], "assistente_id": r["assistente_id"],
        "esecuzione_id": v["esecuzione_id"], "sessione_id": v["sessione_id"],
        "invio_id": v["invio"]["invio_id"], "turno_id": v["invio"]["turno_id"], "stato": v["invio"]["stato"],
    })
    .to_string())
}

/// `esito_routine`: aspetta (al massimo `ATTESA_MASSIMA_S`) che il turno dell'invio finisca e
/// dà stato e testo della risposta; se è ancora in corso lo dice, e si richiama.
pub async fn esito_routine(s: &Stato, invio_id: &str, attendi_s: u64) -> Esito {
    let invio_id = invio_id.trim().to_string();
    if invio_id.is_empty() {
        return Err("serve «invio_id» (lo dà esegui_routine)".into());
    }
    let fine = Instant::now() + Duration::from_secs(attendi_s.min(ATTESA_MASSIMA_S));
    loop {
        let iid = invio_id.clone();
        let t =
            s.db.esegui(move |c| {
                let invio = xbot_db::invii::leggi(c, &iid)?;
                Ok(match turni::per_invio(c, &iid)? {
                    Some(t) if t.stato != "in_corso" => {
                        let testo = crate::motore::testo_del_turno(c, &invio.sessione_id, &t.id)?;
                        Some((t.stato, Some(testo), invio.sessione_id, t.id))
                    }
                    Some(t) => Some(("in_corso".to_string(), None, invio.sessione_id, t.id)),
                    None => Some((
                        "in_attesa".to_string(),
                        None,
                        invio.sessione_id,
                        String::new(),
                    )),
                })
            })
            .await
            .map_err(|e| e.testo)?;
        if let Some((stato, risposta, sessione, turno)) = t {
            if risposta.is_some() || Instant::now() >= fine {
                return Ok(json!({ "stato": stato, "risposta": risposta, "sessione_id": sessione, "turno_id": turno, "invio_id": invio_id }).to_string());
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

impl ServerCanale {
    async fn esegui(&self, canale: &CanaleAttivo, nome: &str, a: &Map<String, Value>) -> Esito {
        let s = &self.stato;
        let aid = canale.assistente_id.as_deref();
        match nome {
            "leggi_memoria" => strumenti::leggi_memoria(s, aid, arg(a, "percorso")).await,
            "cerca_memoria" => strumenti::memoria_cerca(s, aid, arg(a, "parole")).await,
            "scrivi_quaderno" => {
                strumenti::quaderno_scrivi(s, None, arg(a, "agente"), arg(a, "testo")).await
            }
            "elenco_assistenti" => strumenti::assistenti_elenco(s).await.map(|v| v.to_string()),
            "invia_a_assistente" => {
                let attendi = a
                    .get("attendi_secondi")
                    .and_then(Value::as_u64)
                    .unwrap_or(60);
                invia_a_assistente(s, canale, arg(a, "menzione"), arg(a, "testo"), attendi).await
            }
            "stato_quote" => {
                s.db.esegui(provider::quote)
                    .await
                    .map(|v| Value::Array(v).to_string())
                    .map_err(|e| e.testo)
            }
            "cassaforte_elenco" => strumenti::cassaforte_elenco(s, None).await,
            "esegui_routine" => esegui_routine(s, canale, arg(a, "routine")).await,
            "scrivi_memoria" => {
                crate::mcp_interno::memoria_comune::scrivi(s, &canale.nome, a).await
            }
            "esito_routine" => {
                let attendi = a
                    .get("attendi_secondi")
                    .and_then(Value::as_u64)
                    .unwrap_or(60);
                esito_routine(s, arg(a, "invio_id"), attendi).await
            }
            n if n.starts_with("posta_") => {
                // L'invio chiede il sì dell'utente nella sessione corrente dell'assistente del canale.
                let sessione = match aid {
                    Some(x) => {
                        let x = x.to_string();
                        s.db.esegui(move |c| crate::menzioni::sessione_corrente(c, &x))
                            .await
                            .ok()
                    }
                    None => None,
                };
                let ctx = crate::posta::Contesto {
                    sessione_id: sessione,
                    assistente_id: aid.map(str::to_string),
                    ..Default::default()
                };
                crate::posta::chiama(s, &ctx, n, a, true)
                    .await
                    .unwrap_or_else(|| Err(format!("strumento sconosciuto: {n}")))
            }
            _ => Err(format!("strumento sconosciuto: {nome}")),
        }
    }
}

impl ServerHandler for ServerCanale {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(rmcp::model::Implementation::new(
                "xbot-canale",
                xbot_core::VERSIONE,
            ))
            .with_instructions("XBot dell'utente: memoria, quaderni, assistenti e quote.")
    }

    async fn list_tools(
        &self,
        _r: Option<PaginatedRequestParams>,
        c: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let concessi = canale_di(&c).map(|k| k.strumenti).unwrap_or_default();
        let t = definizioni()
            .into_iter()
            .filter(|t| concessi.iter().any(|c| *c == t.name))
            .collect();
        Ok(ListToolsResult::with_all_items(t))
    }

    async fn call_tool(
        &self,
        r: CallToolRequestParams,
        c: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let Some(canale) = canale_di(&c) else {
            return Ok(testo(false, "canale non riconosciuto"));
        };
        let a = r.arguments.unwrap_or_default();
        let nome = r.name.to_string();
        let inizio = Instant::now();
        let (esito, risposta) = if canale.strumenti.contains(&nome) {
            match self.esegui(&canale, &nome, &a).await {
                Ok(t) => ("ok", testo(true, t)),
                Err(t) if t.starts_with(TROPPE) => ("negato", testo(false, t)),
                Err(t) => ("errore", testo(false, t)),
            }
        } else {
            (
                "negato",
                testo(
                    false,
                    format!("lo strumento {nome} non è concesso a questo canale"),
                ),
            )
        };
        let durata = i64::try_from(inizio.elapsed().as_millis()).unwrap_or(i64::MAX);
        let (kid, argomenti) = (canale.id.clone(), privati(&a));
        if let Err(e) = self
            .stato
            .db
            .esegui(move |c| canali::registra_chiamata(c, &kid, &nome, &argomenti, esito, durata))
            .await
        {
            tracing::warn!(errore = %e, "chiamata del canale non registrata");
        }
        Ok(risposta)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn argomenti_privati() {
        let mut a = Map::new();
        a.insert("password".into(), json!("x"));
        a.insert("testo".into(), json!("t".repeat(300)));
        let p = privati(&a);
        assert_eq!(p["password"], "***");
        assert!(p["testo"].as_str().unwrap().ends_with('…'));
        assert_eq!(
            maschera_testo("usa Bearer plc_ab-1 e plc_x9!"),
            "usa Bearer … e plc_…!"
        );
        assert_eq!(maschera_testo("nessun token"), "nessun token");
        let mut b = Map::new();
        b.insert(
            "testo".into(),
            json!({ "dentro": ["chiave plc_segreto99 qui"] }),
        );
        assert_eq!(privati(&b)["testo"]["dentro"][0], "chiave plc_… qui");
        assert_eq!(definizioni().len(), canali::STRUMENTI.len());
    }
}
