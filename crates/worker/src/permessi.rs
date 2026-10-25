//! I permessi del `claude` del Mac: lo stesso disegno di `mcp_interno` del server.
//!
//! Il lavoratore apre il socket Unix `<cartella>/xbot.sock` (0600). Nel `--mcp-config` del
//! turno il server `xbot` è il ponte `xbot-worker mcp-ponte --socket <s> --lavoro <id>`
//! con il gettone del turno nell'ambiente `XBOT_MCP_GETTONE`. Il ponte manda come prima riga
//! `{lavoro, gettone}` e poi copia stdin/stdout. Sul socket risponde un server MCP con il solo
//! strumento `permesso` (`--permission-prompt-tool mcp__xbot__permesso`): ogni chiamata
//! diventa un frame `richiesta` verso il server, e il frame `risolta` la chiude.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::oneshot;
use xbot_core::{Errore, Risposta};

use crate::Uscita;
use crate::protocollo::Corpo;

pub const VARIABILE_GETTONE: &str = "XBOT_MCP_GETTONE";
/// Quanto si aspetta l'utente (10 minuti, come il server).
pub const ATTESA: Duration = Duration::from_secs(600);

/// Come l'ha risolta l'utente (frame `risolta`).
#[derive(Debug, Clone, PartialEq)]
pub struct Risoluzione {
    pub esito: String,
    pub comando_approvato: Option<String>,
    pub motivo: Option<String>,
}

pub struct Permessi {
    pub socket: PathBuf,
    turni: Mutex<HashMap<String, String>>,
    /// Fase 2.G: sessione → (lavoro, turno) in corso, per il ponte di un processo caldo.
    correnti: Mutex<HashMap<String, (String, Option<String>)>>,
    /// richiesta → (lavoro, chi aspetta la risposta dell'utente).
    attese: Mutex<HashMap<String, (String, oneshot::Sender<Risoluzione>)>>,
    uscita: Uscita,
    attesa: Duration,
}

impl std::fmt::Debug for Permessi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Permessi")
            .field("socket", &self.socket)
            .finish_non_exhaustive()
    }
}

fn blocca<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(a) => a.into_inner(),
    }
}

/// Il socket sotto la cartella; se il percorso è troppo lungo (104 byte su macOS), in `/tmp`.
pub fn percorso_socket(cartella: &Path) -> PathBuf {
    let p = cartella.join("xbot.sock");
    if p.as_os_str().len() < 100 {
        return p;
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in p.to_string_lossy().bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    PathBuf::from(format!("/tmp/xbot-worker-{h:016x}.sock"))
}

pub fn consenti(input: &Value) -> Value {
    json!({ "behavior": "allow", "updatedInput": input })
}

pub fn nega(messaggio: impl Into<String>) -> Value {
    json!({ "behavior": "deny", "message": messaggio.into() })
}

/// Accorcia i testi lunghi dell'input (mai tutto un file nella richiesta).
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

/// Tipo e carico della richiesta, nella forma di `mcp_interno::permesso` del server.
pub fn carico_di(nome: &str, input: &Value, turno_id: Option<&str>) -> (String, Value) {
    let campo = |k: &str| input.get(k).and_then(Value::as_str).map(str::to_string);
    let (azione, risorsa, tipo) = match nome {
        "Bash" => ("run-command", campo("command"), "revisione_comando"),
        "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => (
            "write-file",
            campo("file_path").or_else(|| campo("notebook_path")),
            "permesso_locale",
        ),
        "Read" | "Glob" | "Grep" => (
            "read-file",
            campo("file_path").or_else(|| campo("path")),
            "permesso_locale",
        ),
        _ => ("use-app", None, "permesso_locale"),
    };
    let carico = json!({
        "azione": azione, "destinatario": nome, "risorsa": risorsa, "strumento": nome,
        "comando": input.get("command"), "descrizione": input.get("description"),
        "input": accorcia(input), "turno_id": turno_id, "macchina": "mac",
    });
    (tipo.to_string(), carico)
}

impl Permessi {
    pub fn nuovo(cartella: &Path, uscita: Uscita) -> Self {
        Self {
            socket: percorso_socket(cartella),
            turni: Mutex::default(),
            correnti: Mutex::default(),
            attese: Mutex::default(),
            uscita,
            attesa: ATTESA,
        }
    }

    pub fn con_attesa(mut self, d: Duration) -> Self {
        self.attesa = d;
        self
    }

    /// Registra un turno e restituisce il suo gettone (casuale).
    pub fn registra(&self, lavoro_id: &str) -> String {
        let gettone: String = rand::random::<[u8; 24]>()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        blocca(&self.turni).insert(lavoro_id.to_string(), gettone.clone());
        gettone
    }

    pub fn togli(&self, lavoro_id: &str) {
        blocca(&self.turni).remove(lavoro_id);
    }

    /// Fase 2.G: il gettone della sessione (stabile fra i turni: il ponte di un processo caldo
    /// resta collegato) e il lavoro in corso.
    pub fn registra_sessione(&self, sessione: &str, lavoro: &str, turno: Option<&str>) -> String {
        let gettone = blocca(&self.turni)
            .entry(format!("s:{sessione}"))
            .or_insert_with(|| {
                rand::random::<[u8; 24]>()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect()
            })
            .clone();
        blocca(&self.correnti).insert(
            sessione.to_string(),
            (lavoro.to_string(), turno.map(str::to_string)),
        );
        gettone
    }

    /// A fine lavoro: la sessione non ha più un lavoro in corso (il gettone resta).
    pub fn togli_sessione(&self, sessione: &str, lavoro: &str) {
        let mut c = blocca(&self.correnti);
        if c.get(sessione).is_some_and(|(l, _)| l == lavoro) {
            c.remove(sessione);
        }
    }

    /// Il frame `risolta` del server: sveglia chi aspetta. Falso se nessuno aspettava.
    pub fn risolvi(&self, richiesta_id: &str, r: Risoluzione) -> bool {
        match blocca(&self.attese).remove(richiesta_id) {
            Some((_, tx)) => tx.send(r).is_ok(),
            None => false,
        }
    }

    /// 2.L-bis: i lavori fermi su una domanda all'utente (permesso aperto). Non occupano un posto
    /// del Mac: il client aspetta e non lavora (prova vera delle 09:55: due turni di grok fermi
    /// 10 minuti su un permesso tenevano i due posti e un turno Cursor è stato rifiutato).
    pub fn lavori_in_attesa(&self) -> std::collections::HashSet<String> {
        blocca(&self.attese)
            .values()
            .map(|(l, _)| l.clone())
            .collect()
    }

    /// Decide su uno strumento chiedendo all'utente attraverso il server.
    pub async fn decidi(
        &self,
        lavoro_id: &str,
        turno_id: Option<&str>,
        nome: &str,
        input: &Value,
    ) -> Value {
        if nome.starts_with("mcp__xbot__") {
            return consenti(input);
        }
        let (tipo, carico) = carico_di(nome, input, turno_id);
        let richiesta_id = xbot_core::nuovo_id();
        let (tx, rx) = oneshot::channel();
        blocca(&self.attese).insert(richiesta_id.clone(), (lavoro_id.to_string(), tx));
        let corpo = Corpo::Richiesta {
            richiesta_id: richiesta_id.clone(),
            tipo,
            carico,
        };
        if self
            .uscita
            .send((lavoro_id.to_string(), corpo))
            .await
            .is_err()
        {
            blocca(&self.attese).remove(&richiesta_id);
            return nega("lavoratore in chiusura: permesso non chiesto");
        }
        let r = tokio::time::timeout(self.attesa, rx).await;
        blocca(&self.attese).remove(&richiesta_id);
        match r {
            Ok(Ok(r)) => match r.esito.as_str() {
                "approvata" => {
                    let mut nuovo = input.clone();
                    if nome == "Bash"
                        && let Some(c) = r.comando_approvato.filter(|c| !c.trim().is_empty())
                    {
                        nuovo["command"] = Value::from(c);
                    }
                    consenti(&nuovo)
                }
                "ritirata" => nega("Richiesta ritirata: il turno è stato interrotto."),
                "negata" => nega(match r.motivo.filter(|m| !m.is_empty()) {
                    Some(m) => format!("L'utente ha negato il permesso: {m}"),
                    None => "L'utente ha negato il permesso.".into(),
                }),
                _ => nega("L'utente non ha risposto in tempo."),
            },
            _ => nega("L'utente non ha risposto in tempo."),
        }
    }
}

/// Fase 2.G: i permessi ACP di grok sul Mac, con lo stesso giro dello strumento `permesso`.
pub struct DalLavoratore {
    pub p: Arc<Permessi>,
    pub sessione: String,
}

#[async_trait::async_trait]
impl xbot_provider::ChiediPermesso for DalLavoratore {
    async fn chiedi(&self, strumento: &str, input: &Value) -> Result<(), String> {
        let corrente = blocca(&self.p.correnti).get(&self.sessione).cloned();
        let Some((lavoro, turno)) = corrente else {
            return Err("nessun turno in corso per questa sessione".into());
        };
        let v = self
            .p
            .decidi(&lavoro, turno.as_deref(), strumento, input)
            .await;
        if v["behavior"] == "allow" {
            Ok(())
        } else {
            Err(v["message"].as_str().unwrap_or("negato").to_string())
        }
    }
}

#[derive(Deserialize)]
struct Intestazione {
    #[serde(default)]
    lavoro: String,
    /// Fase 2.G: il ponte di un processo caldo si presenta con la sessione.
    #[serde(default)]
    sessione: Option<String>,
    gettone: String,
    #[serde(default)]
    turno: Option<String>,
}

/// Apre il socket (0600) e accetta i ponti.
pub async fn avvia(p: Arc<Permessi>) -> Risposta<()> {
    let sock = p.socket.clone();
    if let Some(d) = sock.parent() {
        std::fs::create_dir_all(d)
            .map_err(|e| Errore::interno(format!("cartella del socket: {e}")))?;
    }
    if sock.exists() {
        if UnixStream::connect(&sock).await.is_ok() {
            return Err(Errore::interno(format!(
                "socket {} già in uso da un altro lavoratore",
                sock.display()
            )));
        }
        let _ = std::fs::remove_file(&sock);
    }
    let ascolto = UnixListener::bind(&sock)
        .map_err(|e| Errore::interno(format!("socket {}: {e}", sock.display())))?;
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o600));
    }
    tokio::spawn(async move {
        loop {
            match ascolto.accept().await {
                Ok((flusso, _)) => {
                    let p = p.clone();
                    tokio::spawn(async move { servi(p, flusso).await });
                }
                Err(e) => {
                    tracing::warn!(errore = %e, "socket dei permessi: accettazione fallita");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        }
    });
    Ok(())
}

async fn servi(p: Arc<Permessi>, flusso: UnixStream) {
    let (lettura, scrittura) = flusso.into_split();
    let mut lettura = BufReader::new(lettura);
    let mut riga = String::new();
    let letta = tokio::time::timeout(Duration::from_secs(10), lettura.read_line(&mut riga)).await;
    let Ok(Ok(n)) = letta else { return };
    if n == 0 || n > 4096 {
        return;
    }
    let Ok(i) = serde_json::from_str::<Intestazione>(riga.trim()) else {
        tracing::warn!("socket dei permessi: intestazione illeggibile");
        return;
    };
    let chiave = match &i.sessione {
        Some(s) => format!("s:{s}"),
        None => i.lavoro.clone(),
    };
    let atteso = blocca(&p.turni).get(&chiave).cloned();
    if atteso.as_deref() != Some(i.gettone.as_str()) {
        tracing::warn!(lavoro = %i.lavoro, "socket dei permessi: lavoro sconosciuto o gettone sbagliato");
        return;
    }
    let server = ServerPermessi {
        p,
        lavoro_id: i.lavoro,
        turno_id: i.turno,
        sessione: i.sessione,
    };
    match server.serve((lettura, scrittura)).await {
        Ok(s) => {
            let _ = s.waiting().await;
        }
        Err(e) => tracing::warn!(errore = %e, "socket dei permessi: sessione MCP fallita"),
    }
}

#[derive(Clone)]
struct ServerPermessi {
    p: Arc<Permessi>,
    lavoro_id: String,
    turno_id: Option<String>,
    sessione: Option<String>,
}

fn strumento_permesso() -> Tool {
    let s = |t: &str| json!({ "type": "string", "description": t });
    let schema = json!({
        "type": "object",
        "properties": { "tool_name": s("strumento"), "input": { "type": "object" }, "tool_use_id": s("id") },
        "required": ["tool_name", "input"],
    });
    let schema: Map<String, Value> = schema.as_object().cloned().unwrap_or_default();
    Tool::new(
        "permesso",
        "Riservato a Claude Code (--permission-prompt-tool): decide se uno strumento può partire.",
        Arc::new(schema),
    )
}

impl ServerHandler for ServerPermessi {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(rmcp::model::Implementation::new(
                "xbot",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions("I permessi di XBot dell'utente sul Mac.")
    }

    async fn list_tools(
        &self,
        _r: Option<PaginatedRequestParams>,
        _c: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(vec![strumento_permesso()]))
    }

    async fn call_tool(
        &self,
        r: CallToolRequestParams,
        _c: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let a = r.arguments.unwrap_or_default();
        if r.name != "permesso" {
            return Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "strumento sconosciuto: {}",
                r.name
            ))])
            .into());
        }
        let nome = a
            .get("tool_name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let input = a.get("input").cloned().unwrap_or(Value::Null);
        let (lavoro, turno) = match &self.sessione {
            Some(s) => match blocca(&self.p.correnti).get(s).cloned() {
                Some(x) => x,
                None => {
                    let v = nega("nessun turno in corso per questa sessione");
                    return Ok(
                        CallToolResult::success(vec![ContentBlock::text(v.to_string())]).into(),
                    );
                }
            },
            None => (self.lavoro_id.clone(), self.turno_id.clone()),
        };
        let esito = self.p.decidi(&lavoro, turno.as_deref(), nome, &input).await;
        Ok(CallToolResult::success(vec![ContentBlock::text(esito.to_string())]).into())
    }
}

/// Il ponte `xbot-worker mcp-ponte`: stdin/stdout di Claude Code ↔ socket del lavoratore.
pub async fn ponte(
    socket: &Path,
    lavoro: &str,
    turno: Option<&str>,
    sessione: Option<&str>,
) -> Risposta<()> {
    let gettone = std::env::var(VARIABILE_GETTONE).unwrap_or_default();
    let flusso = UnixStream::connect(socket)
        .await
        .map_err(|e| Errore::interno(format!("socket {}: {e}", socket.display())))?;
    let (mut dal, mut al) = flusso.into_split();
    let i = json!({ "lavoro": lavoro, "gettone": gettone, "turno": turno, "sessione": sessione });
    al.write_all(format!("{i}\n").as_bytes())
        .await
        .map_err(|e| Errore::interno(format!("intestazione: {e}")))?;
    let su = async {
        let _ = tokio::io::copy(&mut tokio::io::stdin(), &mut al).await;
        let _ = al.shutdown().await;
    };
    let giu = async {
        let mut out = tokio::io::stdout();
        let _ = tokio::io::copy(&mut dal, &mut out).await;
        let _ = out.flush().await;
    };
    tokio::select! {
        () = giu => {}
        () = async { su.await; std::future::pending::<()>().await } => {}
    }
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use tokio::sync::mpsc;

    async fn scrivi(s: &mut tokio::net::unix::OwnedWriteHalf, v: Value) {
        s.write_all(format!("{v}\n").as_bytes()).await.unwrap();
    }

    async fn leggi(l: &mut BufReader<tokio::net::unix::OwnedReadHalf>) -> Value {
        let mut r = String::new();
        l.read_line(&mut r).await.unwrap();
        serde_json::from_str(&r).unwrap()
    }

    /// Un client MCP a mano sul socket: initialize, tools/call permesso, frame `richiesta`
    /// in uscita, `risolta` approvata con comando cambiato.
    #[tokio::test]
    async fn permesso_passa_dal_server() {
        let d = std::env::temp_dir().join(format!("plw-{}", &xbot_core::nuovo_id()[..10]));
        let (tx, mut rx) = mpsc::channel(8);
        let p = Arc::new(Permessi::nuovo(&d, tx).con_attesa(Duration::from_secs(5)));
        avvia(p.clone()).await.unwrap();
        let gettone = p.registra("L1");
        // Gettone sbagliato: il socket si chiude.
        let f = UnixStream::connect(&p.socket).await.unwrap();
        let (l, mut s) = f.into_split();
        scrivi(&mut s, json!({"lavoro": "L1", "gettone": "no"})).await;
        let mut l = BufReader::new(l);
        let mut r = String::new();
        assert_eq!(l.read_line(&mut r).await.unwrap(), 0);

        let f = UnixStream::connect(&p.socket).await.unwrap();
        let (l, mut s) = f.into_split();
        let mut l = BufReader::new(l);
        scrivi(
            &mut s,
            json!({"lavoro": "L1", "gettone": gettone, "turno": "T1"}),
        )
        .await;
        scrivi(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-06-18", "capabilities": {},
            "clientInfo": {"name": "prova", "version": "1"}}}),
        )
        .await;
        let v = leggi(&mut l).await;
        assert_eq!(v["result"]["serverInfo"]["name"], "xbot");
        scrivi(
            &mut s,
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        )
        .await;
        scrivi(
            &mut s,
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
            "name": "permesso", "arguments": {"tool_name": "Bash", "input": {"command": "rm x"}}}}),
        )
        .await;
        let (lavoro, corpo) = rx.recv().await.unwrap();
        assert_eq!(lavoro, "L1");
        let Corpo::Richiesta {
            richiesta_id,
            tipo,
            carico,
        } = corpo
        else {
            panic!("non è una richiesta")
        };
        assert_eq!(tipo, "revisione_comando");
        assert_eq!(carico["comando"], "rm x");
        assert_eq!(carico["turno_id"], "T1");
        assert!(p.risolvi(
            &richiesta_id,
            Risoluzione {
                esito: "approvata".into(),
                comando_approvato: Some("ls".into()),
                motivo: None,
            }
        ));
        let v = leggi(&mut l).await;
        let testo = v["result"]["content"][0]["text"].as_str().unwrap();
        let esito: Value = serde_json::from_str(testo).unwrap();
        assert_eq!(esito["behavior"], "allow");
        assert_eq!(esito["updatedInput"]["command"], "ls");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn senza_risposta_nega() {
        let d = std::env::temp_dir();
        let (tx, mut rx) = mpsc::channel(8);
        let p = Permessi::nuovo(&d, tx).con_attesa(Duration::from_millis(50));
        let h = tokio::spawn(async move { rx.recv().await });
        let v = p
            .decidi("L", None, "Write", &json!({"file_path": "/a"}))
            .await;
        assert_eq!(v["behavior"], "deny");
        assert!(h.await.unwrap().is_some());
        let v = p.decidi("L", None, "mcp__xbot__x", &json!({})).await;
        assert_eq!(v["behavior"], "allow");
    }

    /// 2.L-bis: un lavoro fermo su un permesso dell'utente è «in attesa» (non occupa un posto) finché
    /// L'utente non risponde.
    #[tokio::test]
    async fn lavoro_fermo_sul_permesso() {
        let d = std::env::temp_dir();
        let (tx, mut rx) = mpsc::channel(8);
        let p = Arc::new(Permessi::nuovo(&d, tx).con_attesa(Duration::from_secs(5)));
        assert!(p.lavori_in_attesa().is_empty());
        let p2 = p.clone();
        let h = tokio::spawn(async move {
            p2.decidi("L7", None, "Write", &json!({"file_path": "/a"}))
                .await
        });
        let Some((_, Corpo::Richiesta { richiesta_id, .. })) = rx.recv().await else {
            panic!("nessuna richiesta")
        };
        assert_eq!(
            p.lavori_in_attesa(),
            ["L7".to_string()].into_iter().collect()
        );
        assert!(p.risolvi(
            &richiesta_id,
            Risoluzione {
                esito: "approvata".into(),
                comando_approvato: None,
                motivo: None,
            }
        ));
        assert_eq!(h.await.unwrap()["behavior"], "allow");
        assert!(p.lavori_in_attesa().is_empty());
    }
}
