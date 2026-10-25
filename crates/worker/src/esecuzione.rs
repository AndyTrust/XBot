//! L'esecuzione di un lavoro: la `RichiestaFilo` diventa `RichiestaTurno`, parte l'adattatore
//! di `xbot-provider`, gli eventi vanno nel giornale (attraverso l'uscita) e alla fine si
//! cancella la cartella del turno (`<cartella>/turni/<id>/`, con il `--mcp-config` 0600).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};
use xbot_core::SegretoTesto;
use xbot_provider::claude_code::mcp::{self, ConfigMcp, ServerMcp};
use xbot_provider::{
    ClaudeCode, Codex, CursorAgent, EventoTurno, Finto, Gemini, Grok, Provider, RichiestaTurno,
    codex, cursor_agent, gemini, grok,
};

use crate::permessi::{Permessi, VARIABILE_GETTONE};
use crate::protocollo::{AllegatoFilo, Corpo, EventoFilo, RichiestaFilo};
use crate::{Config, Uscita, scoperta};

/// Il nome del collegamento simbolico al lavoratore che lancia `claude` senza
/// `--strict-mcp-config` (turni con Chrome: il client carica i suoi MCP di utente).
pub const NOME_CLAUDE_UTENTE: &str = "claude-mcp-utente";

/// Un lavoro da eseguire (dal frame `lavoro`).
#[derive(Debug, Clone)]
pub struct Lavoro {
    pub id: String,
    pub generazione: i64,
    pub sessione_id: String,
    pub turno_id: Option<String>,
    pub richiesta: RichiestaFilo,
}

fn segreti(c: &[crate::protocollo::Coppia]) -> Vec<(String, SegretoTesto)> {
    c.iter()
        .map(|x| (x.nome.clone(), x.valore.clone()))
        .collect()
}

/// La cartella del turno sul Mac.
pub fn cartella_turno(conf: &Config, lavoro_id: &str) -> PathBuf {
    conf.cartella.join("turni").join(lavoro_id)
}

/// Un nome di file sicuro: niente cartelle, niente `..`, niente caratteri di controllo.
fn nome_sicuro(nome: &str) -> String {
    let base = nome.rsplit(['/', '\\']).next().unwrap_or("");
    let pulito: String = base
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .trim_start_matches('.')
        .to_string();
    if pulito.is_empty() {
        "allegato".into()
    } else {
        pulito
    }
}

/// Scrive un allegato in `turni/<id>/allegati/<nome>` (0600, cartelle 0700). Due allegati
/// con lo stesso nome: il secondo prende `-2`, `-3`…
pub fn scrivi_allegato(
    conf: &Config,
    lavoro_id: &str,
    nome: &str,
    contenuto_base64: &str,
    usati: &mut std::collections::HashSet<String>,
) -> Result<PathBuf, String> {
    use base64::Engine;
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    let byte = base64::engine::general_purpose::STANDARD
        .decode(contenuto_base64.as_bytes())
        .map_err(|e| format!("base64 non valido: {e}"))?;
    let cartella = cartella_turno(conf, lavoro_id).join("allegati");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&cartella)
        .map_err(|e| e.to_string())?;
    let mut n = nome_sicuro(nome);
    if !usati.insert(n.clone()) {
        let (testa, coda) = match n.rsplit_once('.') {
            Some((t, c)) if !t.is_empty() => (t.to_string(), format!(".{c}")),
            _ => (n.clone(), String::new()),
        };
        let mut i = 2;
        while !usati.insert(format!("{testa}-{i}{coda}")) {
            i += 1;
        }
        n = format!("{testa}-{i}{coda}");
    }
    let p = cartella.join(&n);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&p)
        .map_err(|e| e.to_string())?;
    file.write_all(&byte).map_err(|e| e.to_string())?;
    Ok(p)
}

/// Il permesso della macchina per il turno: `always` | `never` | `ask`. Dal frame
/// (`permesso_esecuzione`); un server che non lo manda dice solo `chiedi_permessi`.
pub fn permesso_della_macchina(f: &RichiestaFilo) -> &'static str {
    match f.permesso_esecuzione.as_deref() {
        Some("always") => "always",
        Some("never") => "never",
        Some(_) => "ask",
        None if f.chiedi_permessi => "ask",
        None => "always",
    }
}

/// Il turno di Claude Code chiede all'utente (strumento `permesso`, con il gettone) solo con
/// «chiedi»: con «sempre» agisce senza chiedere, con «mai» resta in piano (2.L-quater).
pub fn claude_chiede(f: &RichiestaFilo) -> bool {
    f.provider_id == xbot_provider::claude_code::ID && permesso_della_macchina(f) == "ask"
}

/// La `RichiestaTurno` per l'adattatore, più gli avvisi (allegati che sul Mac non ci sono).
pub fn richiesta_turno(
    conf: &Config,
    l: &Lavoro,
    gettone: Option<&str>,
    socket: &Path,
) -> (RichiestaTurno, Vec<String>) {
    let f = &l.richiesta;
    let mut avvisi = Vec::new();
    let mut allegati = Vec::new();
    let mut nomi = std::collections::HashSet::new();
    for a in &f.allegati {
        match a {
            AllegatoFilo::Percorso(p) => {
                let p = PathBuf::from(p);
                if p.is_file() {
                    allegati.push(p);
                } else {
                    avvisi.push(format!("allegato non disponibile sul Mac: {}", p.display()));
                }
            }
            AllegatoFilo::Contenuto {
                nome,
                contenuto_base64,
                ..
            } => match scrivi_allegato(conf, &l.id, nome, contenuto_base64, &mut nomi) {
                Ok(p) => allegati.push(p),
                Err(e) => avvisi.push(format!("allegato {nome} non scritto sul Mac: {e}")),
            },
        }
    }
    let cartella_lavoro = f
        .cartella_lavoro
        .as_deref()
        .map(PathBuf::from)
        .filter(|p| p.is_dir());
    let mut server = Vec::new();
    if let Some(g) = gettone {
        // Fase 2.G: il ponte si presenta con la sessione (gettone stabile): il processo caldo
        // tiene lo stesso `--mcp-config` fra i turni e il lavoro in corso lo sa il lavoratore.
        let argomenti = vec![
            "mcp-ponte".to_string(),
            "--socket".into(),
            socket.to_string_lossy().into_owned(),
            "--sessione".into(),
            l.sessione_id.clone(),
        ];
        server.push(ServerMcp::Stdio {
            nome: mcp::SERVER_INTERNO.into(),
            comando: conf.programma.to_string_lossy().into_owned(),
            argomenti,
            ambiente: vec![(VARIABILE_GETTONE.into(), SegretoTesto::nuovo(g))],
        });
    }
    for c in &f.collegamenti {
        if c.nome == mcp::SERVER_INTERNO {
            continue;
        }
        server.push(match c.trasporto.as_str() {
            "stdio" => ServerMcp::Stdio {
                nome: c.nome.clone(),
                comando: c.comando.clone().unwrap_or_default(),
                argomenti: c.argomenti.clone(),
                ambiente: segreti(&c.ambiente),
            },
            _ => ServerMcp::Http {
                nome: c.nome.clone(),
                url: c.url.clone().unwrap_or_default(),
                intestazioni: segreti(&c.intestazioni),
            },
        });
    }
    let config = (!server.is_empty()).then(|| ConfigMcp {
        file: conf
            .cartella
            .join("sessioni-mcp")
            .join(nome_sicuro(&l.sessione_id))
            .join("mcp-config.json"),
        server,
        strumento_permessi: gettone.map(|_| mcp::STRUMENTO_PERMESSI.to_string()),
    });
    let r = RichiestaTurno {
        sessione_id: l.sessione_id.clone(),
        sessione_esterna_id: f.sessione_esterna_id.clone(),
        modello: f.modello.clone(),
        livello: f.livello.clone(),
        cartella_lavoro,
        testo: f.testo.clone(),
        allegati,
        sistema: f.sistema.clone(),
        // Per Claude Code il modo segue il permesso della macchina (`always` →
        // `bypassPermissions`, `never` → `plan`, `ask` → `default` + strumento `permesso`).
        permessi: if f.provider_id == xbot_provider::claude_code::ID {
            Some(permesso_della_macchina(f).into())
        } else {
            f.permessi.clone()
        },
        mcp: config,
        ambiente: segreti(&f.ambiente),
        chiedi_permesso: None,
    };
    (r, avvisi)
}

/// L'adattatore per un provider. `None` = non eseguibile su questo Mac.
pub fn adattatore(conf: &Config, id: &str, chrome: bool) -> Result<Box<dyn Provider>, String> {
    match id {
        "claude_code" => {
            let programma = if chrome {
                conf.cartella.join("bin").join(NOME_CLAUDE_UTENTE)
            } else {
                scoperta::nel_path("claude").ok_or("claude non è nel PATH del Mac")?
            };
            Ok(Box::new(
                ClaudeCode::nuovo(programma).con_cartella_sessioni(&conf.sessioni),
            ))
        }
        "finto" if conf.consenti_finto => Ok(Box::new(Finto::nuovo())),
        "cursor_agent" => Ok(Box::new(
            CursorAgent::nuovo(
                cursor_agent::trova_nel_path().ok_or("agent non è nel PATH del Mac")?,
            )
            .con_cartella_sessioni(&conf.sessioni),
        )),
        "codex" => Ok(Box::new(
            Codex::nuovo(codex::trova_nel_path().ok_or("codex non è nel PATH del Mac")?)
                .con_cartella_sessioni(&conf.sessioni),
        )),
        "gemini" => Ok(Box::new(
            Gemini::nuovo(gemini::trova_nel_path().ok_or("gemini non è nel PATH del Mac")?)
                .con_cartella_sessioni(&conf.sessioni),
        )),
        "grok" => Ok(Box::new(
            Grok::nuovo(grok::trova_nel_path().ok_or("grok non è nel PATH del Mac")?)
                .con_cartella_sessioni(&conf.sessioni)
                .con_casa(conf.cartella.join("grok-home")),
        )),
        altro => Err(format!(
            "il provider {altro} non ha ancora un adattatore nel lavoratore"
        )),
    }
}

/// Avvia il lavoro in un compito suo. Il `Preso` lo ha già scritto chi chiama; qui arrivano
/// gli eventi e alla fine il `Chiuso`. Il mittente restituito interrompe il turno.
pub fn avvia(
    conf: Arc<Config>,
    l: Lavoro,
    uscita: Uscita,
    permessi: Arc<Permessi>,
) -> oneshot::Sender<()> {
    let (stop_tx, stop_rx) = oneshot::channel();
    tokio::spawn(async move {
        let id = l.id.clone();
        let (stato, motivo) = esegui(&conf, &l, &uscita, &permessi, stop_rx).await;
        permessi.togli(&id);
        permessi.togli_sessione(&l.sessione_id, &id);
        let cartella = cartella_turno(&conf, &id);
        if cartella.exists()
            && let Err(e) = tokio::fs::remove_dir_all(&cartella).await
        {
            tracing::warn!(lavoro = %id, errore = %e, "cartella del turno non cancellata");
        }
        tracing::info!(lavoro = %id, stato = %stato, "lavoro chiuso");
        let _ = uscita.send((id, Corpo::Chiuso { stato, motivo })).await;
    });
    stop_tx
}

async fn manda(uscita: &Uscita, id: &str, e: EventoFilo) {
    let _ = uscita.send((id.to_string(), Corpo::Evento(e))).await;
}

async fn esegui(
    conf: &Config,
    l: &Lavoro,
    uscita: &Uscita,
    permessi: &Arc<Permessi>,
    mut stop: oneshot::Receiver<()>,
) -> (String, Option<String>) {
    let f = &l.richiesta;
    let provider = match adattatore(conf, &f.provider_id, f.chrome) {
        Ok(p) => p,
        Err(t) => {
            manda(
                uscita,
                &l.id,
                EventoFilo::Fallito {
                    codice: "provider_assente".into(),
                    testo: t.clone(),
                },
            )
            .await;
            return ("fallito".into(), Some(t));
        }
    };
    let gettone = claude_chiede(f)
        .then(|| permessi.registra_sessione(&l.sessione_id, &l.id, l.turno_id.as_deref()));
    let (mut richiesta, avvisi) = richiesta_turno(conf, l, gettone.as_deref(), &permessi.socket);
    // Fase 2.G: grok in ACP chiede i permessi al client: vanno all'utente come quelli di Claude.
    if f.provider_id == xbot_provider::grok::ID && f.chiedi_permessi {
        permessi.registra_sessione(&l.sessione_id, &l.id, l.turno_id.as_deref());
        richiesta.chiedi_permesso = Some(xbot_provider::Permessi(Arc::new(
            crate::permessi::DalLavoratore {
                p: permessi.clone(),
                sessione: l.sessione_id.clone(),
            },
        )));
    }
    // L'avviso «questo client non riceve i collegamenti…» lo mette il server.
    for a in avvisi {
        manda(uscita, &l.id, EventoFilo::Avviso { testo: a }).await;
    }
    let (etx, mut erx) = mpsc::channel::<EventoTurno>(64);
    let maniglia = match provider.avvia(richiesta, etx).await {
        Ok(m) => m,
        Err(e) => {
            let t = e.to_string();
            manda(
                uscita,
                &l.id,
                EventoFilo::Fallito {
                    codice: e.codice.come_testo().into(),
                    testo: t.clone(),
                },
            )
            .await;
            return ("fallito".into(), Some(t));
        }
    };
    let mut interrompi = Some(maniglia.interrompi);
    let mut interrotto = false;
    let mut ultimo: Option<EventoFilo> = None;
    loop {
        tokio::select! {
            ev = erx.recv() => match ev {
                Some(e) => {
                    let e = EventoFilo::from(e);
                    ultimo = Some(e.clone());
                    manda(uscita, &l.id, e).await;
                }
                None => break,
            },
            r = &mut stop, if interrompi.is_some() => {
                if r.is_ok() {
                    interrotto = true;
                    if let Some(i) = interrompi.take() {
                        i();
                    }
                } else {
                    // Il mittente è caduto senza interrompere: il turno continua.
                    interrompi = None;
                }
            }
        }
    }
    match ultimo {
        _ if interrotto => ("interrotto".into(), None),
        Some(EventoFilo::Finito { .. }) => ("riuscito".into(), None),
        Some(EventoFilo::Fallito { testo, .. }) => ("fallito".into(), Some(testo)),
        _ => (
            "fallito".into(),
            Some("il client è uscito senza chiudere il turno".into()),
        ),
    }
}

#[cfg(test)]
#[path = "esecuzione_prove.rs"]
mod test;
