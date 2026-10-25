//! Il server MCP interno di XBot (Fase 2.D S2), su socket Unix.
//!
//! Claude Code lo lancia come server stdio `xbot` con il comando
//! `xbot-server mcp-interno --socket <percorso> --sessione <id> --turno <id>`: quel comando
//! è solo un ponte stdio↔socket ([`ponte`]). La prima riga sul socket è l'intestazione
//! `{sessione, turno, gettone}`; il gettone (casuale, per sessione dalla 2.G) arriva al ponte nell'ambiente
//! `XBOT_MCP_GETTONE` scritto nel `--mcp-config` (0600). Il server riconosce il turno dal
//! registro e risponde con gli strumenti `permesso`, `cassaforte_elenco`, `cassaforte_chiedi`,
//! `quaderno_scrivi`, `memoria_cerca`, `assistenti_elenco`, `invia_a`.

pub mod cliente;
pub mod collega;
pub mod memoria_comune;
pub mod figlie;
pub mod lavagna;
pub mod permesso;
pub mod ponte;
pub mod specchio;
pub mod strumenti;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::Duration;

use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixListener;
use xbot_core::{Errore, Risposta};
use xbot_db::cassaforte::Chiave;

use crate::Stato;

/// La variabile d'ambiente con il gettone del turno, per il ponte.
pub const VARIABILE_GETTONE: &str = "XBOT_MCP_GETTONE";
/// Quanto si aspetta l'utente su una richiesta (10 minuti).
pub const ATTESA_RICHIESTA: Duration = Duration::from_secs(600);

/// Un turno con il server interno: chi è, e cosa può usare.
#[derive(Debug, Clone)]
pub struct TurnoMcp {
    pub turno_id: String,
    pub sessione_id: String,
    pub assistente_id: String,
    pub invio_id: String,
    pub gettone: String,
    /// Passi della catena fra assistenti a cui appartiene questo turno (0 = dall'utente).
    pub passo: u32,
    /// La cartella del turno (mcp-config, valori approvati): si cancella a fine turno.
    pub cartella: PathBuf,
    /// (nome nel mcp-config, id del collegamento).
    pub collegamenti: Vec<(String, String)>,
}

/// Lo stato di S2 condiviso dal motore: socket, registro dei turni, chiave della cassaforte.
pub struct StatoS2 {
    pub cartella: PathBuf,
    pub socket: PathBuf,
    programma: RwLock<Option<PathBuf>>,
    chiave: RwLock<Option<Arc<Chiave>>>,
    turni: Mutex<HashMap<String, TurnoMcp>>,
    /// Il gettone di ogni sessione (Fase 2.G): stabile quanto il processo caldo che lo usa.
    gettoni: Mutex<HashMap<String, String>>,
    attesa: RwLock<Duration>,
    voce: RwLock<Arc<crate::voce::ConfigVoce>>,
    attese: RwLock<Option<crate::richieste::Attese>>,
    pub menzioni: crate::menzioni::Limiti,
}

impl std::fmt::Debug for StatoS2 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatoS2")
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

/// Il percorso del socket: `XBOT_SOCKET`, altrimenti `<cartella>/xbot.sock`; un percorso
/// troppo lungo per un socket Unix (104 byte su macOS) va in `/tmp`.
fn percorso_socket(cartella: &Path) -> PathBuf {
    if let Some(p) = std::env::var_os("XBOT_SOCKET").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let p = cartella.join("xbot.sock");
    if p.as_os_str().len() < 100 {
        return p;
    }
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(p.to_string_lossy().as_bytes());
    let corto: String = h.iter().take(6).map(|b| format!("{b:02x}")).collect();
    PathBuf::from(format!("/tmp/xbot-{corto}.sock"))
}

impl StatoS2 {
    /// In una cartella temporanea nuova (finché lo stato non dà quella dei dati).
    pub fn temporaneo() -> Self {
        Self::in_cartella(std::env::temp_dir().join(format!("xbot-s2-{}", xbot_core::nuovo_id())))
    }

    pub fn in_cartella(cartella: PathBuf) -> Self {
        Self {
            socket: percorso_socket(&cartella),
            cartella,
            programma: RwLock::new(None),
            chiave: RwLock::new(None),
            turni: Mutex::default(),
            gettoni: Mutex::default(),
            attesa: RwLock::new(ATTESA_RICHIESTA),
            voce: RwLock::new(Arc::new(crate::voce::ConfigVoce::da_ambiente())),
            attese: RwLock::new(None),
            menzioni: crate::menzioni::Limiti::default(),
        }
    }

    /// Il programma che fa da ponte (`xbot-server`): quello impostato o l'eseguibile corrente.
    pub fn programma(&self) -> PathBuf {
        if let Ok(g) = self.programma.read()
            && let Some(p) = g.as_ref()
        {
            return p.clone();
        }
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("xbot-server"))
    }

    pub fn imposta_programma(&self, p: impl Into<PathBuf>) {
        if let Ok(mut g) = self.programma.write() {
            *g = Some(p.into());
        }
    }

    /// Quanto si aspetta l'utente (10 minuti; le prove lo accorciano).
    pub fn attesa(&self) -> Duration {
        self.attesa.read().map_or(ATTESA_RICHIESTA, |g| *g)
    }

    pub fn imposta_attesa(&self, d: Duration) {
        if let Ok(mut g) = self.attesa.write() {
            *g = d;
        }
    }

    /// La configurazione della voce (dall'ambiente all'avvio).
    pub fn voce(&self) -> Arc<crate::voce::ConfigVoce> {
        match self.voce.read() {
            Ok(g) => g.clone(),
            Err(e) => e.into_inner().clone(),
        }
    }

    pub fn imposta_voce(&self, c: crate::voce::ConfigVoce) {
        if let Ok(mut g) = self.voce.write() {
            *g = Arc::new(c);
        }
    }

    /// Le attese delle richieste di S1 (per svegliare chi aspetta una richiesta ritirata).
    pub fn imposta_attese(&self, a: crate::richieste::Attese) {
        if let Ok(mut g) = self.attese.write() {
            *g = Some(a);
        }
    }

    pub fn attese(&self) -> crate::richieste::Attese {
        self.attese
            .read()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn imposta_chiave(&self, k: Option<Arc<Chiave>>) {
        if let Ok(mut g) = self.chiave.write() {
            *g = k;
        }
    }

    /// La chiave della cassaforte (dai servizi di S1). `None` = cassaforte chiusa.
    pub fn chiave(&self) -> Option<Arc<Chiave>> {
        self.chiave.read().ok().and_then(|g| g.clone())
    }

    /// Toglie il file del socket (alla chiusura del server).
    pub fn togli_socket(&self) {
        match std::fs::remove_file(&self.socket) {
            Ok(()) => tracing::info!(socket = %self.socket.display(), "socket rimosso"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!(errore = %e, "socket non rimosso"),
        }
    }

    pub fn registra(&self, t: TurnoMcp) {
        blocca(&self.turni).insert(t.turno_id.clone(), t);
    }

    pub fn togli(&self, turno_id: &str) -> Option<TurnoMcp> {
        blocca(&self.turni).remove(turno_id)
    }

    pub fn turno(&self, turno_id: &str) -> Option<TurnoMcp> {
        blocca(&self.turni).get(turno_id).cloned()
    }

    /// Il gettone della sessione: lo stesso per tutti i turni (il ponte di un processo caldo
    /// resta collegato fra un turno e l'altro), nuovo alla prima richiesta.
    pub fn gettone_sessione(&self, sessione_id: &str) -> String {
        blocca(&self.gettoni)
            .entry(sessione_id.to_string())
            .or_insert_with(|| {
                rand::random::<[u8; 24]>()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect()
            })
            .clone()
    }

    /// Il turno in corso di una sessione (uno alla volta per sessione).
    pub fn turno_della_sessione(&self, sessione_id: &str) -> Option<TurnoMcp> {
        blocca(&self.turni)
            .values()
            .find(|t| t.sessione_id == sessione_id)
            .cloned()
    }
}

#[derive(Deserialize)]
struct Intestazione {
    sessione: String,
    /// Il turno di apertura (informativo dalla 2.G: il turno vero si cerca a ogni chiamata).
    #[serde(default)]
    turno: String,
    gettone: String,
}

/// Apre il socket e accetta i ponti. Il socket ha permessi 0600.
pub async fn avvia(stato: Stato) -> Risposta<()> {
    tokio::spawn(specchio::ciclo(stato.clone()));
    let s2 = stato.motore.s2().clone();
    let sock = s2.socket.clone();
    if let Some(d) = sock.parent() {
        std::fs::create_dir_all(d)
            .map_err(|e| Errore::interno(format!("cartella del socket: {e}")))?;
    }
    if sock.exists() {
        // Orfano solo se nessuno risponde: un altro server vivo non si scavalca.
        if tokio::net::UnixStream::connect(&sock).await.is_ok() {
            return Err(Errore::interno(format!(
                "socket {} già in uso da un altro server",
                sock.display()
            )));
        }
        let _ = std::fs::remove_file(&sock);
        tracing::info!(socket = %sock.display(), "socket orfano rimosso");
    }
    let ascolto = UnixListener::bind(&sock)
        .map_err(|e| Errore::interno(format!("socket {}: {e}", sock.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o600));
    }
    tracing::info!(socket = %sock.display(), "server MCP interno in ascolto");
    tokio::spawn(async move {
        loop {
            match ascolto.accept().await {
                Ok((flusso, _)) => {
                    let s = stato.clone();
                    tokio::spawn(async move { servi_ponte(s, flusso).await });
                }
                Err(e) => {
                    tracing::warn!(errore = %e, "socket MCP: accettazione fallita");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }
        }
    });
    Ok(())
}

async fn servi_ponte(stato: Stato, flusso: tokio::net::UnixStream) {
    let (lettura, scrittura) = flusso.into_split();
    let mut lettura = BufReader::new(lettura);
    let mut riga = String::new();
    let letta = tokio::time::timeout(Duration::from_secs(10), lettura.read_line(&mut riga)).await;
    let Ok(Ok(n)) = letta else { return };
    if n == 0 || n > 4096 {
        return;
    }
    let Ok(i) = serde_json::from_str::<Intestazione>(riga.trim()) else {
        tracing::warn!("socket MCP: intestazione illeggibile");
        return;
    };
    let s2 = stato.motore.s2();
    let del_turno = s2
        .turno(&i.turno)
        .is_some_and(|t| t.gettone == i.gettone && t.sessione_id == i.sessione);
    let della_sessione = blocca(&s2.gettoni).get(&i.sessione) == Some(&i.gettone);
    if !(del_turno || della_sessione) {
        tracing::warn!(turno = %i.turno, "socket MCP: sessione sconosciuta o gettone sbagliato");
        return;
    }
    let server = ServerInterno {
        stato,
        sessione_id: i.sessione,
    };
    match server.serve((lettura, scrittura)).await {
        Ok(servizio) => {
            let _ = servizio.waiting().await;
        }
        Err(e) => tracing::warn!(errore = %e, "socket MCP: avvio della sessione fallito"),
    }
}

/// Uno strumento: nome, descrizione, proprietà e campi obbligatori dello schema.
pub fn strumento(
    nome: &'static str,
    descrizione: &'static str,
    prop: Value,
    obbligatori: &[&str],
) -> Tool {
    let schema = json!({ "type": "object", "properties": prop, "required": obbligatori });
    let schema: Map<String, Value> = schema.as_object().cloned().unwrap_or_default();
    Tool::new(nome, descrizione, Arc::new(schema))
}

/// Il risultato testuale di uno strumento (errore leggibile dall'assistente se `ok` è falso).
pub fn testo(ok: bool, t: impl Into<String>) -> CallToolResponse {
    let c = vec![ContentBlock::text(t)];
    if ok {
        CallToolResult::success(c)
    } else {
        CallToolResult::error(c)
    }
    .into()
}

fn strumenti_interni() -> Vec<Tool> {
    let s = |t: &str| json!({ "type": "string", "description": t });
    let mut v = vec![
        strumento(
            "permesso",
            "Riservato a Claude Code (--permission-prompt-tool): decide se uno strumento può partire.",
            json!({ "tool_name": s("strumento"), "input": { "type": "object" }, "tool_use_id": s("id") }),
            &["tool_name", "input"],
        ),
        strumento(
            "cassaforte_elenco",
            "Le voci della cassaforte dell'utente: id, genere, titolo e siti. Mai i valori.",
            json!({}),
            &[],
        ),
        strumento(
            "cassaforte_chiedi",
            "Chiede all'utente di usare una voce della cassaforte per questo turno. Se approva, il valore arriva in un file privato da usare in Bash: mai scriverlo in chat.",
            json!({ "id": s("id della voce (da cassaforte_elenco)"), "motivo": s("perché serve") }),
            &["id", "motivo"],
        ),
        strumento(
            "cassaforte_salva",
            "Apre in chat il campo protetto dove l'utente scrive una password, un token o una chiave: il valore va dritto nella cassaforte cifrata, tu ricevi solo l'id. Usalo ogni volta che l'utente vuole salvare o darti una credenziale: non chiederla mai nel testo e non rifiutare.",
            json!({
                "titolo": s("nome della voce, es. «Gmail personale», «Token Hostinger»"),
                "genere": s("«segreto» (token, chiave API: predefinito) o «accesso» (utente e password di un sito)"),
                "variabile": s("per un segreto: nome della variabile, es. HOSTINGER_API_TOKEN (facoltativo)"),
                "sito": s("indirizzo del sito (facoltativo)"),
                "utente": s("per un accesso: utente o email, se lo sai (facoltativo)"),
                "motivo": s("una riga per l'utente: a cosa serve e dove si prende")
            }),
            &["titolo"],
        ),
        strumento(
            "posta_aggiungi_casella",
            "Collega una casella di posta nuova (anche PEC) alla posta di XBot: apre in chat il box protetto per la password, la salva in cassaforte, registra i server e fa una lettura di prova. Usalo quando l'utente dice «collegati alla mail …». Se non sai i server IMAP/SMTP del provider, chiedili.",
            json!({
                "indirizzo": s("l'indirizzo, es. info@azienda.it"),
                "imap_server": s("es. imap.gmail.com, imaps.aruba.it, imaps.pec.aruba.it"),
                "smtp_server": s("es. smtp.gmail.com, smtps.aruba.it, smtps.pec.aruba.it"),
                "imap_porta": { "type": "integer", "description": "predefinita 993" },
                "smtp_porta": { "type": "integer", "description": "predefinita 465" },
                "pec": { "type": "boolean" },
                "azienda": s("a chi appartiene (facoltativo)"),
                "utente": s("utente se diverso dall'indirizzo (facoltativo)"),
                "id": s("nome corto della casella (facoltativo)"),
                "ricollega": { "type": "boolean", "description": "cambia la password di una casella che c'è già" }
            }),
            &["indirizzo", "imap_server", "smtp_server"],
        ),
        strumento(
            "collegamenti_catalogo",
            "I servizi che XBot sa collegare (Google, Microsoft 365, WordPress, Notion, Slack, Stripe, Canva, Meta Ads, Hostinger e molti altri), come si collegano e se l'utente li ha già collegati.",
            json!({ "cerca": s("filtra per nome (facoltativo)") }),
            &[],
        ),
        strumento(
            "collegamento_aggiungi",
            "Collega un servizio per l'utente, tutto dalla chat: lo crea, apre il campo protetto per token o password se servono, ti dà il link del consenso OAuth da passare all'utente, lo prova. Con ricollega=true rifà l'accesso scaduto. Fuori catalogo: {nome, url} di un server MCP remoto, o {nome, comando, argomenti, campi}.",
            json!({
                "voce": s("id della voce del catalogo (da collegamenti_catalogo)"),
                "ricollega": { "type": "boolean", "description": "rifà accesso, token o password anche se c'è già" },
                "nome": s("solo fuori catalogo: nome del collegamento"),
                "url": s("solo fuori catalogo: url https del server MCP"),
                "comando": s("solo fuori catalogo: comando stdio, es. npx"),
                "argomenti": { "type": "array", "items": { "type": "string" } },
                "campi": { "type": "array", "items": { "type": "object" }, "description": "solo fuori catalogo: [{nome: VARIABILE, etichetta, segreto: true}] da chiedere all'utente" }
            }),
            &[],
        ),
        strumento(
            "quaderno_scrivi",
            "Scrive nel tuo quaderno (la memoria che impari). Righe con «DA SALVARE:», «ERRORE DA SALVARE:», «PROPOSTA:» oppure testo libero.",
            json!({ "testo": s("cosa salvare"), "agente": s("nome del quaderno (se manca, il tuo)") }),
            &["testo"],
        ),
        strumento(
            "memoria_cerca",
            "Cerca nella memoria dell'utente (la cartella di note impostata).",
            json!({ "parole": s("parole da cercare") }),
            &["parole"],
        ),
        strumento(
            "assistenti_elenco",
            "Gli altri assistenti di XBot con la loro @menzione e cosa fanno.",
            json!({}),
            &[],
        ),
        strumento(
            "invia_a",
            "Scrive a un altro assistente (come @nome nel testo). La sua risposta torna solo se lui scrive @te.",
            json!({ "menzione": s("@nome dell'assistente"), "testo": s("il messaggio") }),
            &["menzione", "testo"],
        ),
        strumento(
            "apri_figlia",
            "Apre una chat figlia per un lavoro lungo: lavora a parte e ti riporta solo il risultato. Puoi continuare nel frattempo. Con menzione la affidi a un altro assistente.",
            json!({ "compito": s("cosa deve fare, per intero"), "titolo": s("titolo corto"), "menzione": s("@nome dell'assistente (se manca, tu)") }),
            &["compito"],
        ),
        strumento(
            "lavagna_leggi",
            "La lavagna comune: chi sta lavorando su cosa, su quali file, da quanto. Leggila prima di cominciare un lavoro.",
            json!({ "progetto": s("solo questo progetto (facoltativo)") }),
            &[],
        ),
        strumento(
            "lavagna_scrivi",
            "Scrive sulla lavagna cosa stai facendo (progetto, cosa, file). Richiamalo per aggiornare; con finito=true chiudi la tua voce con un esito.",
            json!({
                "progetto": s("progetto o area"),
                "cosa": s("cosa stai facendo, una riga"),
                "file": { "type": "array", "items": { "type": "string" }, "description": "file o cartelle che toccherai" },
                "risorse": { "type": "array", "items": { "type": "string" }, "description": "risorse condivise (tunnel, database, schermo)" },
                "finito": { "type": "boolean" },
                "esito": s("com'è andata (con finito)")
            }),
            &[],
        ),
        strumento(
            "lavagna_prendo_file",
            "Prenota file o cartelle prima di scriverli. Se un altro li ha già presi te lo dice (chi, da quando): decidi se aspettare, chiedergli con @nome o avvisare l'utente.",
            json!({
                "file": { "type": "array", "items": { "type": "string" }, "description": "file o cartelle" },
                "risorse": { "type": "array", "items": { "type": "string" } },
                "progetto": s("progetto (facoltativo)"),
                "cosa": s("cosa farai (facoltativo se hai già una voce)")
            }),
            &["file"],
        ),
    ];
    v.extend(crate::posta::definizioni());
    v
}

/// Il server MCP di una sessione: a ogni chiamata vale il turno in corso (Fase 2.G).
#[derive(Clone)]
pub struct ServerInterno {
    pub stato: Stato,
    pub sessione_id: String,
}

fn argomento<'a>(a: &'a Map<String, Value>, k: &str) -> &'a str {
    a.get(k).and_then(Value::as_str).unwrap_or_default()
}

impl ServerInterno {
    async fn chiama(&self, nome: &str, a: Map<String, Value>) -> CallToolResponse {
        let m = &self.stato;
        let Some(turno) = m.motore.s2().turno_della_sessione(&self.sessione_id) else {
            return testo(false, "nessun turno in corso per questa sessione");
        };
        let t = &turno;
        match nome {
            "permesso" => {
                let r = permesso::decidi(
                    m,
                    t,
                    argomento(&a, "tool_name"),
                    a.get("input").unwrap_or(&Value::Null),
                )
                .await;
                testo(true, r.to_string())
            }
            "cassaforte_elenco" => strumenti::esito(strumenti::cassaforte_elenco(m, Some(t)).await),
            "cassaforte_chiedi" => strumenti::esito(
                strumenti::cassaforte_chiedi(m, t, argomento(&a, "id"), argomento(&a, "motivo"))
                    .await,
            ),
            "cassaforte_salva" => strumenti::esito(collega::cassaforte_salva(m, t, &a).await),
            "collegamenti_catalogo" => {
                strumenti::esito(collega::catalogo_per_bot(m, argomento(&a, "cerca")).await)
            }
            "collegamento_aggiungi" => strumenti::esito(collega::aggiungi(m, t, &a).await),
            "posta_aggiungi_casella" => strumenti::esito(collega::posta_aggiungi(m, t, &a).await),
            "quaderno_scrivi" => strumenti::esito(
                strumenti::quaderno_scrivi(
                    m,
                    Some(t),
                    argomento(&a, "agente"),
                    argomento(&a, "testo"),
                )
                .await,
            ),
            "memoria_cerca" => strumenti::esito(
                strumenti::memoria_cerca(m, Some(&t.assistente_id), argomento(&a, "parole")).await,
            ),
            "assistenti_elenco" => {
                strumenti::esito(strumenti::assistenti_elenco(m).await.map(|v| v.to_string()))
            }
            "apri_figlia" => strumenti::esito(figlie::apri(m, t, &a).await),
            "lavagna_leggi" => strumenti::esito(lavagna::leggi(m, t, &a).await),
            "lavagna_scrivi" => strumenti::esito(lavagna::scrivi(m, t, &a).await),
            "lavagna_prendo_file" => strumenti::esito(lavagna::prendo_file(m, t, &a).await),
            "invia_a" => {
                let r = crate::menzioni::da_strumento(
                    &m.motore,
                    t,
                    argomento(&a, "menzione"),
                    argomento(&a, "testo"),
                )
                .await;
                strumenti::esito(r)
            }
            n if n.starts_with("posta_") => {
                let ctx = crate::posta::Contesto {
                    sessione_id: Some(t.sessione_id.clone()),
                    assistente_id: Some(t.assistente_id.clone()),
                    cartella: Some(t.cartella.clone()),
                    attesa: Some(m.motore.s2().attesa()),
                };
                match crate::posta::chiama(m, &ctx, n, &a, true).await {
                    Some(e) => strumenti::esito(e),
                    None => testo(false, format!("strumento sconosciuto: {n}")),
                }
            }
            _ => testo(false, format!("strumento sconosciuto: {nome}")),
        }
    }
}

impl ServerHandler for ServerInterno {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(rmcp::model::Implementation::new(
                "xbot",
                xbot_core::VERSIONE,
            ))
            .with_instructions(
                "Gli strumenti di XBot dell'utente: cassaforte, quaderno, memoria, colleghi.",
            )
    }

    async fn list_tools(
        &self,
        _r: Option<PaginatedRequestParams>,
        _c: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(strumenti_interni()))
    }

    async fn call_tool(
        &self,
        r: CallToolRequestParams,
        _c: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        Ok(self.chiama(&r.name, r.arguments.unwrap_or_default()).await)
    }
}
