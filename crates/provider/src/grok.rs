//! L'adattatore `grok`: lancia la CLI ufficiale di xAI, Grok Build (`grok`, pacchetto
//! `@xai-official/grok`, abbonamento SuperGrok / X Premium+), in modo headless con
//! `--single=<testo> --output-format streaming-json`, e traduce le righe in `EventoTurno`.
//!
//! Formato dalla documentazione installata con `grok` 1.0.70 (`~/.grok/docs/user-guide/14-headless-mode.md`):
//! una riga JSON per evento con `type` = `text` (`data`: un pezzo di testo), `thought` (`data`),
//! `tool_call` (`toolCallId`, `toolName`, `title`, `kind`, `status`, `rawInput`), `tool_call_update`
//! (`toolCallId`, `status`, `rawOutput`, `content`), `usage` (una per risposta del modello),
//! `plan`, `available_commands`, `max_turns_reached`, `auto_compact_*`, `end` (sempre l'ultima:
//! `stopReason`, `sessionId`, `usage {input_tokens, output_tokens, cache_read_input_tokens}`,
//! `total_cost_usd` solo se completo), `error` (`message`, uscita diversa da 0).
//!
//! Headless `grok` non legge stdin: il testo va in `--single=`, il sistema in `--rules=`. La sessione
//! nuova riceve un UUID scelto qui (`--session-id=`), così l'id c'è già alla prima riga; la ripresa è
//! `--resume=<id>`. La forma `--opzione=valore` evita che un testo che comincia con `-` sembri
//! un'opzione.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::mpsc;
use xbot_core::Errore;

use crate::esterni::{self, Lancio, Permesso, Traduttore, intero};
use crate::{EventoTurno, ManigliaTurno, Modello, Provider, Quota, RichiestaTurno};

pub const ID: &str = "grok";
/// Il modello più economico dell'elenco di `grok models` (usato nelle prove vere).
pub const MODELLO_ECONOMICO: &str = "grok-4.5";
/// I livelli di `--reasoning-effort` quando la sonda non risponde. Il README di grok 1.0.70 elenca
/// `none`…`max`, ma ogni modello accetta solo quelli del suo menu: provato il 2026-10-25,
/// grok-4.7, grok-4.7-build-fast e grok-4.6 `low, medium, high, xhigh`; grok-4.5 `low, medium, high`.
pub const LIVELLI: [&str; 3] = ["low", "medium", "high"];
/// Il livello finto della sonda: grok risponde subito, senza chiamare il modello,
/// «unknown effort level 'xbot-sonda'; use one of: xhigh, high, medium, low».
const SONDA: &str = "xbot-sonda";

/// Gli strumenti che scrivono o eseguono, negati con `--deny` quando il permesso è `never`.
pub const NEGATI_SE_MAI: [&str; 4] = ["Bash", "Edit", "Write", "MCPTool"];

#[derive(Debug, Clone)]
pub struct Grok {
    programma: PathBuf,
    sessioni: PathBuf,
    silenzio: Duration,
    attesa_kill: Duration,
    /// La casa isolata (`GROK_HOME` e `HOME` di grok, Fase 2.G-bis).
    casa: PathBuf,
}

/// `grok` nel `PATH`.
pub fn trova_nel_path() -> Option<PathBuf> {
    esterni::trova_nel_path(&["grok"])
}

impl Grok {
    pub fn nuovo(programma: impl Into<PathBuf>) -> Self {
        Self {
            programma: programma.into(),
            sessioni: crate::claude_code::cartella_sessioni_predefinita(),
            silenzio: Duration::from_secs(600),
            attesa_kill: Duration::from_secs(5),
            casa: casa::predefinita(),
        }
    }

    /// La casa isolata di grok (predefinita: `$XBOT_DATI/grok-home` o `…/worker/grok-home`).
    pub fn con_casa(mut self, cartella: impl Into<PathBuf>) -> Self {
        self.casa = cartella.into();
        self
    }

    /// Prepara la casa e dà le variabili da aggiungere in fondo all'ambiente di grok.
    pub fn ambiente_casa(&self) -> Result<Vec<(String, String)>, Errore> {
        casa::prepara(&self.casa)
            .map_err(|e| Errore::interno(format!("casa di grok {}: {e}", self.casa.display())))
    }

    /// Un comando di servizio (`models`, sonda) nella casa isolata.
    fn comando_servizio(&self) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(&self.programma);
        match self.ambiente_casa() {
            Ok(a) => {
                cmd.envs(a);
            }
            Err(e) => tracing::warn!(errore = %e, "grok senza casa isolata"),
        }
        crate::esterni::pulisci(&mut cmd);
        cmd
    }

    pub fn con_cartella_sessioni(mut self, cartella: impl Into<PathBuf>) -> Self {
        self.sessioni = cartella.into();
        self
    }

    pub fn con_silenzio(mut self, silenzio: Duration) -> Self {
        self.silenzio = silenzio;
        self
    }

    pub fn cartella_di(&self, r: &RichiestaTurno) -> PathBuf {
        esterni::cartella_di(&self.sessioni, r)
    }
}

/// Un UUID v4 in minuscolo (per `--session-id`, che vuole un UUID).
pub fn uuid_nuovo() -> String {
    let mut b = [0u8; 16];
    let letti = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut b))
        .is_ok();
    if !letti {
        // Senza /dev/urandom: ora e processo (basta l'unicità, non serve segretezza).
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
            ^ (u128::from(std::process::id()) << 64);
        b = n.to_le_bytes();
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// Un livello accettabile: una parola semplice (i livelli dei menu dei modelli, come `deep`).
fn livello_valido(l: &str) -> bool {
    !l.is_empty()
        && l.len() <= 32
        && l.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Il testo del turno senza il sistema (che va in `--rules`), con gli allegati come percorsi.
fn testo_grok(r: &RichiestaTurno) -> String {
    let mut t = r.testo.clone();
    for p in &r.allegati {
        t.push_str(&format!("\n\n[allegato: {}]", p.display()));
    }
    t.replace('\0', "")
}

/// Gli argomenti di `grok` per una richiesta. `nuova` è l'UUID della sessione nuova (ignorato
/// quando c'è da riprendere `sessione_esterna_id`).
pub fn argomenti(r: &RichiestaTurno, nuova: &str) -> Vec<OsString> {
    let mut a: Vec<OsString> = vec![
        format!("--single={}", testo_grok(r)).into(),
        "--output-format".into(),
        "streaming-json".into(),
    ];
    match r.sessione_esterna_id.as_deref().filter(|s| !s.is_empty()) {
        Some(id) => a.push(format!("--resume={id}").into()),
        None => a.push(format!("--session-id={nuova}").into()),
    }
    if let Some(m) = r.modello.as_deref().filter(|s| !s.is_empty()) {
        a.push("-m".into());
        a.push(m.into());
    }
    if let Some(l) = r.livello.as_deref().filter(|l| livello_valido(l)) {
        a.push("--reasoning-effort".into());
        a.push(l.into());
    }
    if let Some(s) = r.sistema.as_deref().filter(|s| !s.trim().is_empty()) {
        a.push(format!("--rules={}", s.trim().replace('\0', "")).into());
    }
    match Permesso::di(r) {
        Permesso::Mai => {
            for n in NEGATI_SE_MAI {
                a.push(format!("--deny={n}").into());
            }
            a.push("--permission-mode".into());
            a.push("dontAsk".into());
        }
        // In headless nessuno può rispondere a una domanda: `dontAsk` nega quello che andrebbe
        // chiesto (le letture restano approvate) e il lettore lo dice in un avviso.
        Permesso::Chiedi => {
            a.push("--permission-mode".into());
            a.push("dontAsk".into());
        }
        Permesso::Sempre => a.push("--always-approve".into()),
    }
    a
}

/// Le righe di `grok models`: `  * grok-4.6 (default)` e `  - grok-4.5`.
pub fn leggi_modelli(uscita: &str) -> Vec<Modello> {
    uscita
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("* ").or_else(|| l.strip_prefix("- ")))
        .filter_map(|resto| {
            let mut parti = resto.split_whitespace();
            let id = parti.next()?.to_string();
            let predefinito = resto.contains("(default)");
            Some(Modello {
                etichetta: etichetta_di(&id),
                dettaglio: match (predefinito, id.ends_with("-fast")) {
                    (true, _) => Some("predefinito".to_string()),
                    (false, true) => Some("fast".to_string()),
                    _ => None,
                },
                livelli: LIVELLI.iter().map(|l| l.to_string()).collect(),
                id,
            })
        })
        .collect()
}

/// `grok-4.6` → `Grok 4.6`.
fn etichetta_di(id: &str) -> String {
    let mut c = id.replace('-', " ");
    if let Some(p) = c.get_mut(0..1) {
        p.make_ascii_uppercase();
    }
    c
}

fn modelli_fissi() -> Vec<Modello> {
    leggi_modelli("  * grok-4.7 (default)\n  - grok-4.7-build-fast\n  - grok-4.6\n  - grok-4.5\n")
}

/// I livelli dal messaggio della sonda, dal più leggero: `use one of: xhigh, high, medium, low`.
pub fn leggi_livelli(messaggio: &str) -> Vec<String> {
    let Some((_, resto)) = messaggio.split_once("use one of:") else {
        return Vec::new();
    };
    let mut v: Vec<String> = resto
        .lines()
        .next()
        .unwrap_or_default()
        .split(',')
        .map(|x| x.trim().trim_end_matches(['"', '.', '}']).to_string())
        .filter(|x| livello_valido(x))
        .collect();
    v.reverse();
    v
}

/// Vero se il testo dice che la CLI non ha un accesso valido.
fn non_autenticato(t: &str) -> bool {
    let t = t.to_lowercase();
    t.contains("not signed in") || t.contains("not authenticated") || t.contains("grok login")
}

/// Vero se l'esito di uno strumento dice che il permesso è stato negato.
fn negato(v: &Value) -> bool {
    let t = v.to_string().to_lowercase();
    [
        "permission",
        "denied",
        "not allowed",
        "rejected",
        "cancelled the execution",
    ]
    .iter()
    .any(|p| t.contains(p))
}

/// Lo stato della lettura di un turno di `grok`.
#[derive(Debug, Default)]
pub struct Lettore {
    nomi: HashMap<String, (String, Value)>,
    sessione: Option<String>,
    avviato: bool,
    /// Vero dopo uno strumento negato: il `cancelled` dell'`end` che segue non è un'interruzione.
    negato: bool,
    finito: bool,
}

impl Lettore {
    /// `sessione` = l'id passato con `--session-id` o `--resume`.
    pub fn nuovo(sessione: Option<String>) -> Self {
        Self {
            sessione,
            ..Self::default()
        }
    }

    pub fn sessione(&self) -> Option<&str> {
        self.sessione.as_deref()
    }

    fn chiamata(&mut self, v: &Value) -> Vec<EventoTurno> {
        let id = v["toolCallId"].as_str().unwrap_or_default().to_string();
        let nome = v["toolName"]
            .as_str()
            .or(v["title"].as_str())
            .unwrap_or("strumento")
            .to_string();
        let mut out = vec![EventoTurno::ChiamataStrumento {
            nome: nome.clone(),
            input_json: v["rawInput"].clone(),
        }];
        self.nomi.insert(id.clone(), (nome, v["rawInput"].clone()));
        // Una chiamata che arriva già chiusa.
        if matches!(v["status"].as_str(), Some("completed" | "failed")) {
            out.extend(self.aggiornamento(v));
        }
        out
    }

    fn aggiornamento(&mut self, v: &Value) -> Vec<EventoTurno> {
        let stato = v["status"].as_str().unwrap_or_default();
        if stato != "completed" && stato != "failed" {
            return Vec::new();
        }
        let id = v["toolCallId"].as_str().unwrap_or_default();
        let (nome, input) = self
            .nomi
            .remove(id)
            .unwrap_or_else(|| ("strumento".to_string(), Value::Null));
        let esito = if v["rawOutput"].is_null() {
            v["content"].clone()
        } else {
            v["rawOutput"].clone()
        };
        let ok = stato == "completed";
        let mut out = vec![EventoTurno::EsitoStrumento {
            nome: nome.clone(),
            esito_json: esito.clone(),
            ok,
        }];
        if !ok && negato(&esito) {
            self.negato = true;
            let cosa = input["command"]
                .as_str()
                .map(|c| format!("il comando «{c}»"))
                .unwrap_or_else(|| format!("lo strumento «{nome}»"));
            out.push(EventoTurno::Avviso(format!(
                "Grok ha negato {cosa}: senza «sempre» nel permesso della macchina gli strumenti \
                 che scrivono non partono"
            )));
        }
        out
    }

    fn fine(&mut self, v: &Value) -> Vec<EventoTurno> {
        self.finito = true;
        if let Some(s) = v["sessionId"].as_str().filter(|s| !s.is_empty()) {
            self.sessione = Some(s.to_string());
        }
        let motivo = v["stopReason"].as_str().unwrap_or("end_turn");
        let mut out = Vec::new();
        // Un comando negato in headless arriva come `tool_call_update` `failed` («User cancelled the
        // execution for tool …») e l'`end` dice `cancelled` (stderr: «permission required: … was not
        // run, as a headless run cannot ask for approval»): il turno resta riuscito, con l'avviso.
        // Le chiamate rimaste aperte (senza `tool_call_update`) si chiudono qui.
        let mut aperte: Vec<(String, (String, Value))> = self.nomi.drain().collect();
        aperte.sort_by(|a, b| a.0.cmp(&b.0));
        let negate = motivo == "cancelled" && !aperte.is_empty();
        let negato_prima = self.negato;
        for (_, (nome, input)) in aperte {
            out.push(EventoTurno::EsitoStrumento {
                nome: nome.clone(),
                esito_json: Value::Null,
                ok: !negate,
            });
            if negate {
                let cosa = input["command"]
                    .as_str()
                    .map(|c| format!("il comando «{c}»"))
                    .unwrap_or_else(|| format!("lo strumento «{nome}»"));
                out.push(EventoTurno::Avviso(format!(
                    "Grok ha negato {cosa}: in headless non può chiedere il permesso e senza \
                     «sempre» nel permesso della macchina gli strumenti che scrivono non partono"
                )));
            }
        }
        match motivo {
            "cancelled" if negate || negato_prima => {}
            "cancelled" => {
                return vec![EventoTurno::Fallito {
                    codice: "interrotto".into(),
                    testo: "Grok ha interrotto il turno".into(),
                }];
            }
            "end_turn" | "EndTurn" => {}
            "max_tokens" => out.push(EventoTurno::Avviso(
                "Grok si è fermato al limite di lunghezza della risposta".into(),
            )),
            "max_turn_requests" => out.push(EventoTurno::Avviso(
                "Grok si è fermato al numero massimo di giri".into(),
            )),
            "refusal" => out.push(EventoTurno::Avviso("Grok ha rifiutato la richiesta".into())),
            altro => out.push(EventoTurno::Avviso(format!(
                "Grok ha chiuso il turno con «{altro}»"
            ))),
        }
        let u = &v["usage"];
        out.push(EventoTurno::Finito {
            token_ingresso: intero(&u["input_tokens"]),
            token_uscita: intero(&u["output_tokens"]),
            token_cache: intero(&u["cache_read_input_tokens"]),
            costo_usd: v["total_cost_usd"].as_f64(),
            sessione_esterna_id: self.sessione.clone(),
        });
        out
    }

    fn errore(&mut self, v: &Value) -> Vec<EventoTurno> {
        self.finito = true;
        let msg = v["message"]
            .as_str()
            .unwrap_or("grok ha chiuso il turno con un errore");
        if non_autenticato(msg) {
            return vec![EventoTurno::Fallito {
                codice: "non_autenticato".into(),
                testo: "grok non è autenticato su questo computer: serve `grok login` dal \
                        Terminale (abbonamento SuperGrok o X Premium+)"
                    .into(),
            }];
        }
        vec![EventoTurno::Fallito {
            codice: "errore".into(),
            testo: msg.to_string(),
        }]
    }
}

impl Traduttore for Lettore {
    fn riga(&mut self, linea: &str) -> Vec<EventoTurno> {
        let linea = linea.trim();
        if linea.is_empty() {
            return Vec::new();
        }
        let Ok(v) = serde_json::from_str::<Value>(linea) else {
            tracing::debug!(lunghezza = linea.len(), "riga non JSON ignorata");
            return Vec::new();
        };
        let tipo = v["type"].as_str().unwrap_or_default();
        let mut out = Vec::new();
        if !self.avviato && tipo != "error" {
            self.avviato = true;
            out.push(EventoTurno::Avviato {
                sessione_esterna_id: self.sessione.clone(),
            });
        }
        out.extend(match tipo {
            "text" => v["data"]
                .as_str()
                .filter(|t| !t.is_empty())
                .map(|t| vec![EventoTurno::TestoDelta(t.to_string())])
                .unwrap_or_default(),
            "thought" => v["data"]
                .as_str()
                .filter(|t| !t.is_empty())
                .map(|t| vec![EventoTurno::Ragionamento(t.to_string())])
                .unwrap_or_default(),
            "tool_call" => self.chiamata(&v),
            "tool_call_update" => self.aggiornamento(&v),
            "max_turns_reached" => vec![EventoTurno::Avviso(
                "Grok ha raggiunto il numero massimo di giri".into(),
            )],
            "end" => self.fine(&v),
            "error" => self.errore(&v),
            _ => Vec::new(),
        });
        out
    }

    fn finito(&self) -> bool {
        self.finito
    }
}

impl Grok {
    /// I livelli veri di un modello: `grok` con un livello inesistente risponde con l'elenco
    /// giusto prima di chiamare il modello (nessun costo). `None` se non risponde così.
    async fn sonda_livelli(&self, modello: &str) -> Option<Vec<String>> {
        let mut cmd = self.comando_servizio();
        cmd.arg("--single=x")
            .args(["--output-format", "streaming-json", "-m", modello])
            .args(["--reasoning-effort", SONDA])
            .current_dir(std::env::temp_dir())
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        let o = tokio::time::timeout(Duration::from_secs(15), cmd.output())
            .await
            .ok()?
            .ok()?;
        let tutto = format!(
            "{}\n{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        Some(leggi_livelli(&tutto)).filter(|l| !l.is_empty())
    }
}

#[async_trait]
impl Provider for Grok {
    fn id(&self) -> &str {
        ID
    }

    fn nome(&self) -> &str {
        "Grok (xAI)"
    }

    fn programma(&self) -> &str {
        self.programma.to_str().unwrap_or("grok")
    }

    /// Da `grok models` (20 s al massimo; risponde anche senza accesso); se non va, un elenco fisso.
    async fn modelli(&self) -> Result<Vec<Modello>, Errore> {
        let mut cmd = self.comando_servizio();
        cmd.arg("models")
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        let uscita = tokio::time::timeout(Duration::from_secs(20), cmd.output()).await;
        let m = match uscita {
            Ok(Ok(o)) if o.status.success() => leggi_modelli(&String::from_utf8_lossy(&o.stdout)),
            _ => Vec::new(),
        };
        let mut m = if m.is_empty() { modelli_fissi() } else { m };
        for x in &mut m {
            if let Some(l) = self.sonda_livelli(&x.id).await {
                x.livelli = l;
            }
        }
        Ok(m)
    }

    async fn account(&self) -> Result<Option<crate::Account>, Errore> {
        crate::accesso::account_grok().await
    }

    fn comando_accesso(&self) -> Option<crate::ComandoAccesso> {
        Some(crate::ComandoAccesso::nuovo(
            &self.programma,
            &["login", "--device-auth"],
        ))
    }

    fn comando_uscita(&self) -> Option<crate::ComandoAccesso> {
        Some(crate::ComandoAccesso::nuovo(&self.programma, &["logout"]))
    }

    async fn quota(&self) -> Result<Option<Quota>, Errore> {
        Ok(None)
    }

    async fn avvia(
        &self,
        richiesta: RichiestaTurno,
        eventi: mpsc::Sender<EventoTurno>,
    ) -> Result<ManigliaTurno, Errore> {
        if crate::caldi::accesi() {
            return self.avvia_acp(richiesta, eventi).await;
        }
        let nuova = uuid_nuovo();
        let sessione = richiesta
            .sessione_esterna_id
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| nuova.clone());
        let lancio = Lancio {
            nome: "grok",
            programma: self.programma.clone(),
            argomenti: argomenti(&richiesta, &nuova),
            cartella: self.cartella_di(&richiesta),
            ambiente: richiesta.ambiente.clone(),
            ambiente_adattatore: crate::mcp_altri::ambiente_grok(&self.casa, &richiesta)?,
            stdin: None,
            silenzio: self.silenzio,
            attesa_kill: self.attesa_kill,
        };
        esterni::avvia(lancio, Lettore::nuovo(Some(sessione)), eventi).await
    }
}

/// Gli argomenti di `grok agent … stdio` (Fase 2.G).
pub fn argomenti_acp(r: &RichiestaTurno) -> Vec<OsString> {
    let mut a: Vec<OsString> = vec!["agent".into()];
    if Permesso::di(r) == Permesso::Sempre {
        a.push("--always-approve".into());
    }
    if let Some(m) = r.modello.as_deref().filter(|s| !s.is_empty()) {
        a.push("-m".into());
        a.push(m.into());
    }
    a.push("stdio".into());
    a
}

impl Grok {
    /// Il turno sul processo caldo `grok agent stdio` della sessione (ACP).
    async fn avvia_acp(
        &self,
        richiesta: RichiestaTurno,
        eventi: mpsc::Sender<EventoTurno>,
    ) -> Result<ManigliaTurno, Errore> {
        use crate::caldi;
        let cartella = self.cartella_di(&richiesta);
        let livello = richiesta.livello.clone().filter(|l| livello_valido(l));
        let mut firma = caldi::Firma::default()
            .con(&self.programma)
            .con(&cartella)
            .con(&richiesta.modello)
            .con(&livello)
            .con(Permesso::di(&richiesta) as u8)
            .con(&richiesta.sistema)
            .con(crate::mcp_altri::firma(&richiesta));
        for (k, v) in &richiesta.ambiente {
            firma = firma.con(k).con(v.esponi());
        }
        let firma = firma.fine();
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
        let lavoro = caldi::Lavoro {
            messaggio: acp::Messaggio {
                testo: testo_grok(&richiesta),
                permesso: Permesso::di(&richiesta),
                chiedi: richiesta.chiedi_permesso.clone(),
            },
            eventi,
            stop: stop_rx,
        };
        let lavoro = match caldi::consegna(&richiesta.sessione_id, ID, firma, lavoro) {
            Ok(()) => {
                return Ok(caldi::maniglia(
                    caldi::pid_di(&richiesta.sessione_id, ID),
                    stop_tx,
                ));
            }
            Err(l) => l,
        };
        let lancio = Lancio {
            nome: "grok",
            programma: self.programma.clone(),
            argomenti: argomenti_acp(&richiesta),
            cartella: cartella.clone(),
            ambiente: richiesta.ambiente.clone(),
            ambiente_adattatore: crate::mcp_altri::ambiente_grok(&self.casa, &richiesta)?,
            stdin: None,
            silenzio: self.silenzio,
            attesa_kill: self.attesa_kill,
        };
        let figlio = esterni::lancia(&lancio).await?;
        let pid = figlio.id();
        let apertura = acp::Apertura {
            cartella,
            riprendi: richiesta
                .sessione_esterna_id
                .clone()
                .filter(|s| !s.is_empty()),
            regole: richiesta.sistema.clone(),
            livello,
        };
        let processo = acp::Processo::nuovo(figlio, apertura, self.silenzio, self.attesa_kill)
            .ok_or_else(|| Errore::interno("stdout di grok non disponibile"))?;
        caldi::apri(
            &richiesta.sessione_id,
            ID,
            firma,
            caldi::nuovo_numero(),
            pid,
            Vec::new(),
            processo,
            lavoro,
        );
        Ok(caldi::maniglia(pid, stop_tx))
    }
}

#[path = "grok_casa.rs"]
pub mod casa;

#[path = "grok_acp.rs"]
pub mod acp;

#[cfg(test)]
#[path = "grok_prove.rs"]
mod prove;
