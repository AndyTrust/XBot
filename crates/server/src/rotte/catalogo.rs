//! `GET /collegamenti/catalogo` (Fase 2.I, completato nella 2.K): ogni voce ha la sua strada fino
//! al consenso. `richiede_oauth`: consenso nel browser; `richiede_token`: il modulo chiede solo
//! «Token»; `richiede_client`: prima del consenso serve un client registrato dall'utente in XBot
//! (`PUT …/oauth/client`), con le istruzioni in `client_istruzioni`; `client_nel_server`: il client
//! dell'utente va nel server MCP (Google), XBot si registra da solo; `macchina`: il genere del
//! computer dove gira (stdio).

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, header};
use serde_json::{Value, json};

use crate::stato::Stato;

/// La cartella di note della voce «memoria»: `XBOT_MEMORIA_CARTELLA`, predefinita `~/Memoria`
/// (`~/` diventa la casa della macchina dove il comando parte).
fn cartella_memoria() -> String {
    std::env::var("XBOT_MEMORIA_CARTELLA")
        .ok()
        .filter(|c| !c.trim().is_empty())
        .unwrap_or_else(|| "~/Memoria".into())
}

/// Il nome della voce «memoria»: `XBOT_MEMORIA_NOME`, predefinito «Memoria».
fn nome_memoria() -> &'static str {
    static NOME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    NOME.get_or_init(|| {
        std::env::var("XBOT_MEMORIA_NOME")
            .ok()
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "Memoria".into())
    })
}

/// Dove risponde il server Google ospitato accanto a XBot (`ops/google-mcp/`).
pub const GOOGLE_PERCORSO: &str = "/google-mcp/mcp";

/// Una voce del catalogo.
struct Voce {
    id: &'static str,
    nome: &'static str,
    descrizione: &'static str,
    categoria: &'static str,
    trasporto: &'static str,
    comando: Option<&'static str>,
    argomenti: Vec<String>,
    url: Option<String>,
    richiede_oauth: bool,
    richiede_token: bool,
    richiede_client: bool,
    /// 2.K: il client sta nel server MCP (Google ospitato), non in XBot: niente `PUT …/client`.
    client_nel_server: bool,
    client_istruzioni: Option<String>,
    macchina: Option<&'static str>,
    /// 2.L: l'interfaccia nasconde comando e argomenti (voce «dei nostri» già pronta).
    tecnico: bool,
    nota: Option<&'static str>,
    /// Il logo in `web/loghi/<id>.png` (falso: l'icona generica).
    logo: bool,
    /// Cosa chiedere all'utente prima di collegare (token, password, indirizzi), e dove va.
    campi: Vec<Campo>,
}

/// Un valore da chiedere all'utente per una voce: va nell'ambiente (stdio) o in un'intestazione (http).
#[derive(Clone)]
pub struct Campo {
    pub nome: &'static str,
    pub etichetta: &'static str,
    pub segreto: bool,
    /// Dove si prende (una riga per l'utente).
    pub aiuto: Option<&'static str>,
    /// `Some(prefisso)`: intestazione `Authorization` con il prefisso; `None`: variabile d'ambiente.
    pub intestazione: Option<&'static str>,
}

impl Campo {
    const fn env(nome: &'static str, etichetta: &'static str, segreto: bool) -> Self {
        Campo {
            nome,
            etichetta,
            segreto,
            aiuto: None,
            intestazione: None,
        }
    }

    const fn aiuto(mut self, a: &'static str) -> Self {
        self.aiuto = Some(a);
        self
    }
}

impl Voce {
    fn stdio(id: &'static str, nome: &'static str, descrizione: &'static str) -> Self {
        Voce {
            id,
            nome,
            descrizione,
            categoria: "nostri",
            trasporto: "stdio",
            comando: Some("npx"),
            argomenti: Vec::new(),
            url: None,
            richiede_oauth: false,
            richiede_token: false,
            richiede_client: false,
            client_nel_server: false,
            client_istruzioni: None,
            macchina: None,
            tecnico: false,
            nota: None,
            logo: true,
            campi: Vec::new(),
        }
    }

    fn npx(
        id: &'static str,
        nome: &'static str,
        descrizione: &'static str,
        pacchetto: &str,
    ) -> Self {
        Voce {
            categoria: "terzi",
            argomenti: vec!["-y".into(), pacchetto.into()],
            ..Voce::stdio(id, nome, descrizione)
        }
    }

    fn con_nota(mut self, n: &'static str) -> Self {
        self.nota = Some(n);
        self
    }

    fn http(id: &'static str, nome: &'static str, descrizione: &'static str, url: String) -> Self {
        Voce {
            categoria: "terzi",
            trasporto: "http",
            comando: None,
            url: Some(url),
            richiede_oauth: true,
            ..Voce::stdio(id, nome, descrizione)
        }
    }
}

/// La base pubblica di XBot: `XBOT_URL_PUBBLICO`, altrimenti l'host della richiesta.
pub fn base_pubblica(s: &Stato, h: &HeaderMap) -> String {
    s.servizi
        .url_pubblico
        .clone()
        .or_else(|| {
            h.get(header::HOST)
                .and_then(|v| v.to_str().ok())
                .map(|host| format!("http://{host}"))
        })
        .unwrap_or_else(|| "http://localhost".into())
        .trim_end_matches('/')
        .to_string()
}

/// Gli scope utente che Slack dichiara (`scopes_supported`, letti il 2026-10-25): XBot li chiede
/// tutti, quindi l'app dell'utente li deve avere tutti, sennò Slack risponde `invalid_scope`.
const SCOPE_SLACK: &str = "canvases:read, canvases:write, channels:history, channels:read, \
    channels:write, chat:write, emoji:read, files:read, files:write, groups:history, groups:read, \
    groups:write, im:history, im:read, im:write, lists:read, lists:write, mpim:history, mpim:read, \
    mpim:write, reactions:read, reactions:write, search:read.files, search:read.im, \
    search:read.mpim, search:read.private, search:read.public, search:read.users, users:read, \
    users:read.email";

fn istruzioni_slack(base: &str) -> String {
    format!(
        "1. Apri api.slack.com/apps → «Create New App» → «From scratch», scegli il workspace.\n\
         2. «OAuth & Permissions» → «Redirect URLs» → aggiungi {base}/api/v1/oauth/ritorno e salva.\n\
         3. Sempre lì, «User Token Scopes» (non «Bot»): aggiungi tutti questi, XBot li chiede \
         insieme: {SCOPE_SLACK}.\n\
         4. «Basic Information» → «App Credentials»: copia Client ID e Client Secret qui sotto, \
         poi «Collega»."
    )
}

fn istruzioni_google(base: &str) -> String {
    format!(
        "Il client Google non va in XBot ma nel server Google accanto (XBot si registra da solo \
         presso di lui, poi il server porta l'utente da Google).\n\
         1. console.cloud.google.com → un progetto → «API e servizi» → «Libreria»: abilita Gmail \
         API, Google Drive API e Google Calendar API.\n\
         2. «Schermata consenso OAuth»: tipo Esterno, il tuo indirizzo fra gli utenti di prova.\n\
         3. «Credenziali» → «Crea credenziali» → «ID client OAuth» → «Applicazione web».\n\
         4. «URI di reindirizzamento autorizzati»: {base}/google-mcp/oauth2callback (solo questo).\n\
         5. Client ID e Client Secret nel file google-mcp.env del server Google, con \
         WORKSPACE_EXTERNAL_URL={base}/google-mcp e \
         WORKSPACE_MCP_ALLOWED_CLIENT_REDIRECT_URIS={base}/api/v1/oauth/ritorno \
         (ops/google-mcp/LEGGIMI.md); poi qui «Collega»."
    )
}

fn voci(base: &str) -> Vec<Voce> {
    let fs = |cartella: &str| {
        vec![
            "-y".to_string(),
            "@modelcontextprotocol/server-filesystem".to_string(),
            cartella.to_string(),
        ]
    };
    let mut slack = Voce::http(
        "slack",
        "Slack",
        "Canali e messaggi di Slack.",
        "https://mcp.slack.com/mcp".into(),
    );
    slack.richiede_client = true;
    slack.client_istruzioni = Some(istruzioni_slack(base));
    slack.nota =
        Some("Slack non accetta la registrazione automatica: serve un'app Slack dell'utente");
    let mut google = Voce::http(
        "google",
        "Google (Gmail, Drive, Calendar)",
        "Gmail, Drive e Calendar dal server Google ospitato accanto a XBot.",
        format!("{base}{GOOGLE_PERCORSO}"),
    );
    google.client_nel_server = true;
    google.client_istruzioni = Some(istruzioni_google(base));
    google.nota = Some("serve il server Google acceso accanto a XBot (ops/google-mcp)");
    let mut github = Voce::http(
        "github",
        "GitHub",
        "Repository, issue e pull request.",
        "https://api.githubcopilot.com/mcp/".into(),
    );
    (github.richiede_oauth, github.richiede_token) = (false, true);
    github.nota = Some(
        "il token si crea su github.com → Settings → Developer settings → Personal access tokens",
    );
    github.campi = vec![Campo {
        intestazione: Some("Bearer "),
        ..Campo::env("Authorization", "Token di GitHub", true)
            .aiuto("https://github.com/settings/personal-access-tokens")
    }];
    let h = |id, nome, descrizione, url: &str| Voce::http(id, nome, descrizione, url.into());
    let mut microsoft = Voce::npx(
        "microsoft-365",
        "Microsoft 365",
        "Outlook (posta e calendario), OneDrive, Excel, OneNote, To Do; con un account di lavoro \
         anche Teams e SharePoint.",
        "@softeria/ms-365-mcp-server",
    )
    .con_nota(
        "l'accesso si fa dalla chat: il bot chiama «login» e ti dà un codice da aprire su \
         microsoft.com/devicelogin",
    );
    microsoft.argomenti.push("--org-mode".into());
    let mut wordpress = Voce::npx(
        "wordpress",
        "WordPress (il tuo sito)",
        "Articoli, pagine, media e impostazioni di un sito WordPress tuo.",
        "@automattic/mcp-wordpress-remote",
    )
    .con_nota("sul sito serve il plugin «MCP Adapter» di WordPress (gratuito)");
    wordpress.campi = vec![
        Campo::env("WP_API_URL", "Indirizzo del sito (https://…)", false),
        Campo::env("WP_API_USERNAME", "Utente WordPress", false),
        Campo::env("WP_API_PASSWORD", "Password applicazione", true)
            .aiuto("WordPress → Utenti → Profilo → «Password per le applicazioni» → Aggiungi"),
    ];
    let mut hostinger = Voce::npx(
        "hostinger",
        "Hostinger",
        "Domini, DNS, hosting, VPS e fatture di Hostinger.",
        "hostinger-api-mcp",
    );
    hostinger.campi = vec![
        Campo::env("HOSTINGER_API_TOKEN", "Token API di Hostinger", true)
            .aiuto("hPanel → Profilo → API → «Crea token»"),
    ];
    let mut hf = h(
        "huggingface",
        "Hugging Face",
        "Modelli, dataset e Spaces di Hugging Face.",
        "https://huggingface.co/mcp",
    );
    hf.richiede_oauth = false;
    let mut learn = h(
        "microsoft-learn",
        "Microsoft Learn",
        "La documentazione ufficiale Microsoft e Azure, senza account.",
        "https://learn.microsoft.com/api/mcp",
    );
    learn.richiede_oauth = false;
    let mut context7 = h(
        "context7",
        "Context7",
        "Documentazione aggiornata di librerie e framework per programmare.",
        "https://mcp.context7.com/mcp",
    );
    context7.richiede_oauth = false;
    // Server ufficiali con OAuth e registrazione automatica, provati il 2026-10-25 (401 con
    // `registration_endpoint`): basta «Collega» e il consenso nel browser.
    let ufficiali = vec![
        h(
            "linear",
            "Linear",
            "Issue, progetti e cicli di Linear.",
            "https://mcp.linear.app/mcp",
        ),
        h(
            "atlassian",
            "Jira e Confluence",
            "Ticket di Jira e pagine di Confluence (Atlassian).",
            "https://mcp.atlassian.com/v1/mcp",
        ),
        h(
            "monday",
            "monday.com",
            "Bacheche, elementi e aggiornamenti di monday.com.",
            "https://mcp.monday.com/mcp",
        ),
        h(
            "clickup",
            "ClickUp",
            "Attività, liste e documenti di ClickUp.",
            "https://mcp.clickup.com/mcp",
        ),
        h(
            "todoist",
            "Todoist",
            "Attività e progetti di Todoist.",
            "https://ai.todoist.net/mcp",
        ),
        h(
            "airtable",
            "Airtable",
            "Basi, tabelle e record di Airtable.",
            "https://mcp.airtable.com/mcp",
        ),
        h(
            "miro",
            "Miro",
            "Lavagne e oggetti di Miro.",
            "https://mcp.miro.com/",
        ),
        h(
            "dropbox",
            "Dropbox",
            "File e cartelle di Dropbox.",
            "https://mcp.dropbox.com/mcp",
        ),
        h(
            "canva",
            "Canva",
            "Progetti e design di Canva.",
            "https://mcp.canva.com/mcp",
        ),
        h(
            "figma",
            "Figma",
            "File, componenti e commenti di Figma.",
            "https://mcp.figma.com/mcp",
        ),
        h(
            "gamma",
            "Gamma",
            "Presentazioni e documenti di Gamma.",
            "https://mcp.gamma.app/mcp",
        ),
        h(
            "wordpress-com",
            "WordPress.com",
            "Siti ospitati su WordPress.com: articoli, pagine, statistiche.",
            "https://public-api.wordpress.com/wpcom/v2/mcp/v1",
        ),
        h(
            "wix",
            "Wix",
            "Siti Wix: pagine, negozio, prenotazioni.",
            "https://mcp.wix.com/mcp",
        ),
        h(
            "webflow",
            "Webflow",
            "Siti e CMS di Webflow.",
            "https://mcp.webflow.com/mcp",
        ),
        h(
            "meta-ads",
            "Meta Ads",
            "Campagne pubblicitarie di Facebook e Instagram.",
            "https://mcp.facebook.com/ads",
        ),
        h(
            "klaviyo",
            "Klaviyo",
            "Email marketing, liste e campagne di Klaviyo.",
            "https://mcp.klaviyo.com/mcp",
        ),
        h(
            "intercom",
            "Intercom",
            "Conversazioni e contatti di Intercom.",
            "https://mcp.intercom.com/mcp",
        ),
        h(
            "attio",
            "Attio",
            "Il CRM Attio: contatti, aziende, trattative.",
            "https://mcp.attio.com/mcp",
        ),
        h(
            "stripe",
            "Stripe",
            "Pagamenti, clienti, abbonamenti e fatture di Stripe.",
            "https://mcp.stripe.com",
        ),
        h(
            "paypal",
            "PayPal",
            "Transazioni, fatture e controversie di PayPal.",
            "https://mcp.paypal.com/mcp",
        ),
        h(
            "square",
            "Square",
            "Vendite, cataloghi e clienti di Square.",
            "https://mcp.squareup.com/mcp",
        ),
        h(
            "fireflies",
            "Fireflies",
            "Riunioni registrate e trascrizioni di Fireflies.",
            "https://api.fireflies.ai/mcp",
        ),
        h(
            "amplitude",
            "Amplitude",
            "Analisi di prodotto e grafici di Amplitude.",
            "https://mcp.amplitude.com/mcp",
        ),
        h(
            "zapier",
            "Zapier",
            "Le azioni di Zapier su migliaia di app.",
            "https://mcp.zapier.com/api/mcp/mcp",
        ),
        h(
            "vercel",
            "Vercel",
            "Progetti, deploy e log di Vercel.",
            "https://mcp.vercel.com",
        ),
        h(
            "netlify",
            "Netlify",
            "Siti e deploy di Netlify.",
            "https://mcp.netlify.com/mcp",
        ),
        h(
            "supabase",
            "Supabase",
            "Progetti, database e funzioni di Supabase.",
            "https://mcp.supabase.com/mcp",
        ),
        h(
            "sentry",
            "Sentry",
            "Errori e prestazioni delle app su Sentry.",
            "https://mcp.sentry.dev/mcp",
        ),
    ];
    let mut v = vec![
        Voce {
            argomenti: fs(&cartella_memoria()),
            macchina: Some("mac"),
            logo: false,
            ..Voce::stdio(
                "memoria",
                nome_memoria(),
                "La memoria dell'utente: le note della sua cartella (XBOT_MEMORIA_CARTELLA).",
            )
        },
        Voce::http(
            "granola",
            "Granola",
            "Le riunioni registrate con Granola: note, trascrizioni, decisioni.",
            "https://mcp.granola.ai/mcp".into(),
        ),
        google,
        slack,
        Voce::http(
            "notion",
            "Notion",
            "Pagine e database di Notion.",
            "https://mcp.notion.com/mcp".into(),
        ),
        github,
        Voce {
            comando: Some("sh"),
            argomenti: vec![
                "-c".into(),
                "exec \"$HOME/.reason/bin/reason-network\" mcp".into(),
            ],
            macchina: Some("mac"),
            nota: Some(
                "gira sul Mac, dove c'è la rete di Reason; da fuori si scrive alla sessione di XBot sulla rete",
            ),
            ..Voce::stdio(
                "reason-network",
                "Rete di Reason",
                "Parla con gli altri agenti dell'utente sulla rete: sessioni di Claude Code, Grok, Cursor e altri client (chi c'è, manda un messaggio, posta in arrivo).",
            )
        },
        microsoft,
        wordpress,
        hostinger,
    ];
    v.extend(ufficiali);
    v.extend([hf, learn, context7]);
    v.push(Voce {
        argomenti: fs("/tmp"),
        logo: false,
        ..Voce::stdio(
            "filesystem-tmp",
            "File (prova)",
            "Un collegamento di prova sui file di /tmp.",
        )
    });
    v
}

/// La voce http del catalogo con questo url (Google si riconosce dal percorso, la base cambia).
fn voce_per_url<'a>(v: &'a [Voce], url: &str) -> Option<&'a Voce> {
    v.iter().find(|x| {
        x.url.as_deref() == Some(url) || (x.id == "google" && url.ends_with(GOOGLE_PERCORSO))
    })
}

/// Vero se l'url è quello di una voce OAuth del catalogo (il collegamento nasce con
/// `richiede_oauth`).
pub fn richiede_oauth(url: Option<&str>) -> bool {
    let v = voci("");
    url.and_then(|u| voce_per_url(&v, u))
        .is_some_and(|x| x.richiede_oauth)
}

/// 2.K: `richiede_client` e `client_istruzioni` anche nelle righe di `GET /collegamenti`.
pub fn arricchisci(base: &str, riga: &mut Value) {
    let v = voci(base);
    let voce = riga["url"].as_str().and_then(|u| voce_per_url(&v, u));
    if let Some(o) = riga.as_object_mut() {
        o.insert(
            "richiede_client".into(),
            json!(voce.is_some_and(|x| x.richiede_client)),
        );
        o.insert(
            "client_nel_server".into(),
            json!(voce.is_some_and(|x| x.client_nel_server)),
        );
        o.insert(
            "client_istruzioni".into(),
            json!(voce.and_then(|x| x.client_istruzioni.clone())),
        );
    }
}

/// Il catalogo in JSON (per la rotta e per lo strumento `collegamenti_catalogo` dei bot).
pub fn elenco(base: &str) -> Vec<Value> {
    voci(base)
        .into_iter()
        .map(|v| {
            let campi: Vec<Value> = v
                .campi
                .iter()
                .map(|c| {
                    json!({ "nome": c.nome, "etichetta": c.etichetta, "segreto": c.segreto,
                            "aiuto": c.aiuto, "intestazione": c.intestazione.is_some() })
                })
                .collect();
            json!({
                "id": v.id, "nome": v.nome, "descrizione": v.descrizione,
                "categoria": v.categoria, "trasporto": v.trasporto, "comando": v.comando,
                "argomenti": v.argomenti, "url": v.url, "richiede_oauth": v.richiede_oauth,
                "richiede_token": v.richiede_token, "richiede_client": v.richiede_client,
                "client_nel_server": v.client_nel_server,
                "client_istruzioni": v.client_istruzioni, "macchina": v.macchina, "tecnico": v.tecnico,
                "nota": v.nota,
                "logo": v.logo.then(|| format!("loghi/{}.png", v.id)),
                "campi": campi,
            })
        })
        .collect()
}

/// I campi da chiedere per la voce `id` del catalogo.
pub fn campi_di(id: &str) -> Vec<Campo> {
    voci("")
        .into_iter()
        .find(|v| v.id == id)
        .map(|v| v.campi)
        .unwrap_or_default()
}

pub async fn catalogo(State(s): State<Stato>, h: HeaderMap) -> Json<Value> {
    Json(json!({ "catalogo": elenco(&base_pubblica(&s, &h)) }))
}
