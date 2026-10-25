//! La configurazione MCP di un turno (Fase 2.D, S2): il server interno di XBot più i
//! collegamenti abilitati per l'assistente. Diventa il file `--mcp-config` di Claude Code,
//! scritto con permessi 0600 (può contenere l'ambiente decifrato dei collegamenti).

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use xbot_core::{Errore, SegretoTesto};

/// Il nome del server interno nel `--mcp-config`: gli strumenti diventano `mcp__xbot__*`.
pub const SERVER_INTERNO: &str = "xbot";
/// Lo strumento che risponde alle richieste di permesso di Claude Code.
pub const STRUMENTO_PERMESSI: &str = "mcp__xbot__permesso";

/// Un server MCP del turno.
#[derive(Debug, Clone)]
pub enum ServerMcp {
    /// Un programma lanciato da Claude Code, con l'ambiente già decifrato.
    Stdio {
        nome: String,
        comando: String,
        argomenti: Vec<String>,
        ambiente: Vec<(String, SegretoTesto)>,
    },
    /// Un server raggiunto in HTTP, con le intestazioni già decifrate.
    Http {
        nome: String,
        url: String,
        intestazioni: Vec<(String, SegretoTesto)>,
    },
}

impl ServerMcp {
    pub fn nome(&self) -> &str {
        match self {
            ServerMcp::Stdio { nome, .. } | ServerMcp::Http { nome, .. } => nome,
        }
    }
}

/// La configurazione MCP di un turno.
#[derive(Debug, Clone, Default)]
pub struct ConfigMcp {
    /// Dove scrivere il file (fuori dalla cartella di lavoro: l'assistente non lo legge).
    pub file: PathBuf,
    pub server: Vec<ServerMcp>,
    /// `--permission-prompt-tool`: di norma [`STRUMENTO_PERMESSI`].
    pub strumento_permessi: Option<String>,
}

fn mappa(coppie: &[(String, SegretoTesto)]) -> Value {
    let m: Map<String, Value> = coppie
        .iter()
        .map(|(k, v)| (k.clone(), Value::from(v.esponi())))
        .collect();
    Value::Object(m)
}

/// Il JSON `{"mcpServers": {...}}` che Claude Code legge.
pub fn json_config(c: &ConfigMcp) -> Value {
    let mut server = Map::new();
    for s in &c.server {
        let voce = match s {
            ServerMcp::Stdio {
                comando,
                argomenti,
                ambiente,
                ..
            } => json!({
                "type": "stdio",
                "command": crate::prova_mcp::espandi_casa(comando),
                "args": argomenti.iter().map(|a| crate::prova_mcp::espandi_casa(a)).collect::<Vec<_>>(),
                "env": mappa(ambiente),
            }),
            ServerMcp::Http {
                url, intestazioni, ..
            } => json!({ "type": "http", "url": url, "headers": mappa(intestazioni) }),
        };
        server.insert(s.nome().to_string(), voce);
    }
    json!({ "mcpServers": server })
}

/// Scrive il file con permessi 0600 fin dalla creazione (crea la cartella se manca).
pub async fn scrivi(c: &ConfigMcp) -> Result<(), Errore> {
    let testo = json_config(c).to_string();
    let file = c.file.clone();
    tokio::task::spawn_blocking(move || scrivi_privato(&file, testo.as_bytes()))
        .await
        .map_err(|e| Errore::interno(format!("scrittura del mcp-config interrotta: {e}")))?
}

pub(crate) fn scrivi_privato(file: &Path, dati: &[u8]) -> Result<(), Errore> {
    use std::io::Write;
    let errore = |e: std::io::Error| Errore::interno(format!("{}: {e}", file.display()));
    if let Some(d) = file.parent() {
        std::fs::create_dir_all(d).map_err(errore)?;
    }
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let mut f = o.open(file).map_err(errore)?;
    f.write_all(dati).map_err(errore)
}

/// Un nome di variabile d'ambiente accettabile: `[A-Za-z_][A-Za-z0-9_]*`.
pub fn variabile_valida(nome: &str) -> bool {
    let mut c = nome.chars();
    matches!(c.next(), Some(p) if p.is_ascii_alphabetic() || p == '_')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

/// Le variabili che i segreti non possono toccare (come `cassaforte_generi::variabile_vietata`).
pub fn variabile_vietata(nome: &str) -> bool {
    const ESATTE: [&str; 4] = ["PATH", "HOME", "LD_PRELOAD", "LD_LIBRARY_PATH"];
    const PREFISSI: [&str; 4] = ["DYLD_", "XBOT_", "ANTHROPIC_", "CLAUDE_"];
    ESATTE.contains(&nome) || PREFISSI.iter().any(|p| nome.starts_with(p))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn json_con_stdio_e_http() {
        let c = ConfigMcp {
            file: PathBuf::from("/x/mcp-config.json"),
            server: vec![
                ServerMcp::Stdio {
                    nome: "xbot".into(),
                    comando: "/bin/xbot-server".into(),
                    argomenti: vec!["mcp-interno".into()],
                    ambiente: vec![("GETTONE".into(), SegretoTesto::nuovo("g"))],
                },
                ServerMcp::Http {
                    nome: "odoo".into(),
                    url: "https://o/mcp".into(),
                    intestazioni: vec![("Authorization".into(), SegretoTesto::nuovo("Bearer t"))],
                },
            ],
            strumento_permessi: Some(STRUMENTO_PERMESSI.into()),
        };
        let v = json_config(&c);
        assert_eq!(v["mcpServers"]["xbot"]["type"], "stdio");
        assert_eq!(v["mcpServers"]["xbot"]["args"][0], "mcp-interno");
        assert_eq!(v["mcpServers"]["xbot"]["env"]["GETTONE"], "g");
        assert_eq!(
            v["mcpServers"]["odoo"]["headers"]["Authorization"],
            "Bearer t"
        );
        assert!(
            format!("{c:?}").contains("***"),
            "i segreti non escono nel Debug"
        );
    }

    #[test]
    fn variabili() {
        assert!(variabile_valida("XBOT_CASSAFORTE_X1"));
        assert!(variabile_valida("_a"));
        assert!(!variabile_valida("1A"));
        assert!(!variabile_valida("A-B"));
        assert!(!variabile_valida(""));
    }

    #[tokio::test]
    async fn file_privato() {
        let d = std::env::temp_dir().join(format!("xbot-mcp-{}", xbot_core::nuovo_id()));
        let c = ConfigMcp {
            file: d.join("t/mcp-config.json"),
            ..Default::default()
        };
        scrivi(&c).await.unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&c.file).unwrap()).unwrap();
        assert!(v["mcpServers"].as_object().unwrap().is_empty());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let m = std::fs::metadata(&c.file).unwrap().permissions().mode();
            assert_eq!(m & 0o777, 0o600);
        }
        std::fs::remove_dir_all(&d).unwrap();
    }
}
