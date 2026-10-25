//! Il lancio con MCP (Fase 2.D S2): per ogni turno di Claude Code il motore prepara la cartella
//! del turno, il `--mcp-config` (server interno + collegamenti dell'assistente), l'ambiente con i
//! segreti dell'assistente, e registra il turno per il socket. A fine turno toglie tutto e
//! inoltra le menzioni del testo dell'assistente.

use std::sync::Arc;

use xbot_core::{Risposta, StatoTurno};
use xbot_db::{Connessione, cassaforte_voci, collegamenti, invii, turni, tutte_json};
use xbot_provider::claude_code::{self, mcp};

use super::{Lancio, Motore};

// --- 2.I --- rinnovo OAuth prima del turno.
#[path = "oauth_turno.rs"]
mod oauth_turno;
// --- 2.K --- dove girano i collegamenti: avviso per quelli di un'altra macchina.
#[path = "dove_turno.rs"]
mod dove_turno;
// --- 2.L --- i collegamenti anche nei turni di Grok e Cursor.
#[path = "mcp_altri.rs"]
pub(crate) mod mcp_altri;
use crate::mcp_interno::{StatoS2, TurnoMcp, VARIABILE_GETTONE};

/// Il nome di un collegamento nel `--mcp-config`: `[a-z0-9_-]`, unico, mai `xbot`.
pub fn nome_mcp(nome: &str, gia: &[String]) -> String {
    let mut base: String = nome
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    // 2.L-bis: niente `__` nel nome: Claude Code e Grok chiamano gli strumenti
    // `mcp__<server>__<strumento>` / `<server>__<strumento>` (`Posta (IMAP)` → `posta_imap`).
    while base.contains("__") {
        base = base.replace("__", "_");
    }
    base = base.trim_matches('_').to_string();
    if base.is_empty() || base == mcp::SERVER_INTERNO {
        base = format!("c_{base}");
    }
    let mut n = base.clone();
    let mut i = 2;
    while gia.contains(&n) {
        n = format!("{base}_{i}");
        i += 1;
    }
    n
}

/// Il testo dell'assistente in un turno (tutte le sue bolle, in ordine).
pub fn testo_del_turno(c: &Connessione, sessione_id: &str, turno_id: &str) -> Risposta<String> {
    let righe = tutte_json(
        c,
        "SELECT corpo_json FROM riga WHERE sessione_id = ?1 AND genere = 'messaggio'
           AND ruolo = 'assistente' AND json_extract(corpo_json, '$.turno_id') = ?2
           AND soppressa = 0 ORDER BY seq",
        [sessione_id, turno_id],
    )?;
    Ok(righe
        .iter()
        .filter_map(|r| r["corpo_json"]["testo"].as_str())
        .filter(|t| !t.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n"))
}

impl Motore {
    /// Lo stato di S2 (socket, turni, chiave della cassaforte, limiti delle menzioni).
    pub fn s2(&self) -> &Arc<StatoS2> {
        &self.s2
    }

    pub fn db(&self) -> &xbot_db::Db {
        &self.db
    }

    pub fn bus(&self) -> &crate::bus::Bus {
        &self.bus
    }

    /// Prepara il turno con MCP (Claude Code; dalla 2.L i collegamenti anche per Grok e Cursor).
    /// Un errore non ferma il turno: parte senza.
    pub(super) async fn s2_prepara(&self, l: &mut Lancio) {
        if l.turno.provider_id == xbot_provider::grok::ID {
            self.s2_prepara_grok(l).await;
        }
        if l.turno.provider_id != claude_code::ID {
            if xbot_provider::riceve_collegamenti(&l.turno.provider_id) {
                self.s2l_collegamenti(l).await;
            }
            return;
        }
        let srv = self.macchine.server_id();
        self.s2k_prepara(l, &srv).await;
        let s2 = self.s2.clone();
        let chiave = s2.chiave();
        let (aid, sid, iid) = (
            l.assistente_id.clone(),
            l.turno.sessione_id.clone(),
            l.invio_id.clone(),
        );
        let r = self
            .db
            .esegui(move |c| {
                let coll = collegamenti::per_assistente_su(c, chiave.as_deref(), &aid, &srv, true)?;
                let segreti = match chiave.as_deref() {
                    Some(k) => cassaforte_voci::segreti_per_assistente(c, k, &aid, Some(&sid))?,
                    None => Vec::new(),
                };
                let origine = invii::leggi(c, &iid)?.origine;
                Ok((coll, segreti, origine))
            })
            .await;
        let (coll, segreti, origine) = match r {
            Ok(x) => x,
            Err(e) => {
                tracing::error!(errore = %e, "configurazione MCP non preparata: turno senza MCP");
                return;
            }
        };
        // Il gettone e il `--mcp-config` sono della sessione (Fase 2.G): il processo caldo li tiene
        // fra i turni; il turno corrente il server lo trova dalla sessione.
        let gettone = s2.gettone_sessione(&l.turno.sessione_id);
        let cartella = s2.cartella.join("turni").join(&l.turno.id);
        let mut server = vec![mcp::ServerMcp::Stdio {
            nome: mcp::SERVER_INTERNO.into(),
            comando: s2.programma().to_string_lossy().into_owned(),
            argomenti: vec![
                "mcp-interno".into(),
                "--socket".into(),
                s2.socket.to_string_lossy().into_owned(),
                "--sessione".into(),
                l.turno.sessione_id.clone(),
            ],
            ambiente: vec![(
                VARIABILE_GETTONE.into(),
                xbot_core::SegretoTesto::nuovo(gettone.clone()),
            )],
        }];
        let (altri, voci) = mcp_altri::server_dei_collegamenti(coll);
        server.extend(altri);
        l.richiesta.mcp = Some(mcp::ConfigMcp {
            file: s2
                .cartella
                .join("sessioni")
                .join(&l.turno.sessione_id)
                .join("mcp-config.json"),
            server,
            strumento_permessi: Some(mcp::STRUMENTO_PERMESSI.into()),
        });
        l.richiesta.ambiente = segreti;
        s2.registra(TurnoMcp {
            turno_id: l.turno.id.clone(),
            sessione_id: l.turno.sessione_id.clone(),
            assistente_id: l.assistente_id.clone(),
            invio_id: l.invio_id.clone(),
            gettone,
            passo: crate::menzioni::passo_da_origine(&origine),
            cartella,
            collegamenti: voci,
        });
    }

    /// Fase 2.G: grok in ACP chiede i permessi al client; il turno si registra come quelli di
    /// Claude Code e le richieste passano dallo strumento `permesso` del socket.
    async fn s2_prepara_grok(&self, l: &mut Lancio) {
        let s2 = self.s2.clone();
        let iid = l.invio_id.clone();
        let origine = match self
            .db
            .esegui(move |c| Ok(invii::leggi(c, &iid)?.origine))
            .await
        {
            Ok(o) => o,
            Err(e) => {
                tracing::error!(errore = %e, "turno di grok senza permessi dall'utente");
                return;
            }
        };
        let gettone = s2.gettone_sessione(&l.turno.sessione_id);
        s2.registra(TurnoMcp {
            turno_id: l.turno.id.clone(),
            sessione_id: l.turno.sessione_id.clone(),
            assistente_id: l.assistente_id.clone(),
            invio_id: l.invio_id.clone(),
            gettone: gettone.clone(),
            passo: crate::menzioni::passo_da_origine(&origine),
            cartella: s2.cartella.join("turni").join(&l.turno.id),
            collegamenti: Vec::new(),
        });
        l.richiesta.chiedi_permesso = Some(xbot_provider::Permessi(Arc::new(
            crate::mcp_interno::cliente::PermessoDalSocket {
                socket: s2.socket.clone(),
                sessione_id: l.turno.sessione_id.clone(),
                gettone,
            },
        )));
    }

    /// Dopo la chiusura: toglie il turno dal registro, cancella la sua cartella e inoltra le
    /// menzioni del testo dell'assistente (solo se il turno è riuscito).
    pub(super) async fn s2_dopo_turno(&self, sessione_id: &str, l: &Lancio) {
        if let Some(t) = self.s2.togli(&l.turno.id)
            && let Err(e) = tokio::fs::remove_dir_all(&t.cartella).await
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(errore = %e, "cartella del turno non cancellata");
        }
        self.s2_ritira(sessione_id, &l.turno.id).await;
        let (sid, tid, iid) = (
            sessione_id.to_string(),
            l.turno.id.clone(),
            l.invio_id.clone(),
        );
        let r = self
            .db
            .esegui(move |c| {
                let t = turni::leggi(c, &tid)?;
                if t.stato != StatoTurno::Riuscito.come_testo() {
                    return Ok(None);
                }
                let origine = invii::leggi(c, &iid)?.origine;
                Ok(Some((testo_del_turno(c, &sid, &tid)?, origine)))
            })
            .await;
        if let Ok(Some((testo, origine))) = r
            && !crate::menzioni::trova(&testo).is_empty()
        {
            let (m, sid, aid) = (
                self.clone(),
                sessione_id.to_string(),
                l.assistente_id.clone(),
            );
            let passo = crate::menzioni::passo_da_origine(&origine);
            tokio::spawn(async move {
                crate::menzioni::da_assistente(&m, &sid, &aid, passo, &testo).await
            });
        }
    }
}

impl Motore {
    /// A fine turno (interrotto, fallito o anche riuscito) nessuna richiesta del turno resta
    /// aperta: diventano `ritirata` e chi aspetta si sveglia. Se il turno non è riuscito, le
    /// righe degli strumenti rimaste `in_corso` chiudono con esito «interrotto».
    async fn s2_ritira(&self, sessione_id: &str, turno_id: &str) {
        let tid = turno_id.to_string();
        let sid = sessione_id.to_string();
        let r = self
            .db
            .esegui(move |c| {
                let aperte = tutte_json(
                    c,
                    "SELECT id FROM richiesta WHERE stato = 'aperta'
                       AND json_extract(carico_json, '$.turno_id') = ?1",
                    [&tid],
                )?;
                let stato = turni::leggi(c, &tid)?.stato;
                let mut seq = None;
                if stato != StatoTurno::Riuscito.come_testo() {
                    let righe = tutte_json(
                        c,
                        "SELECT id, corpo_json FROM riga WHERE sessione_id = ?1
                           AND genere = 'chiamata_strumento'
                           AND json_extract(corpo_json, '$.turno_id') = ?2
                           AND json_extract(corpo_json, '$.stato') = 'in_corso'",
                        [&sid, &tid],
                    )?;
                    for r in righe {
                        let mut corpo = r["corpo_json"].clone();
                        corpo["stato"] = "interrotto".into();
                        corpo["esito"] = "interrotto".into();
                        corpo["ok"] = false.into();
                        let id = r["id"].as_str().unwrap_or_default();
                        seq = Some(xbot_db::righe::riscrivi(c, id, &corpo)?.aggiornata_seq);
                    }
                }
                Ok((aperte, seq))
            })
            .await;
        let (aperte, seq) = match r {
            Ok(x) => x,
            Err(e) => {
                tracing::warn!(errore = %e, "richieste del turno non ritirate");
                return;
            }
        };
        if let Some(seq) = seq {
            self.bus.pubblica(
                sessione_id,
                crate::bus::Segnale::Righe {
                    aggiornata_seq: seq,
                },
            );
        }
        let attese = self.s2.attese();
        for a in aperte {
            let id = a["id"].as_str().unwrap_or_default();
            match crate::richieste::ritira(&self.db, &self.bus, &attese, id).await {
                Ok(_) => tracing::info!(richiesta = %id, "richiesta ritirata a fine turno"),
                Err(e) => tracing::debug!(richiesta = %id, errore = %e, "richiesta non ritirata"),
            }
        }
    }
}

// --- 2.H ---
/// Il nome del server MCP di Chrome nel `--mcp-config` dei turni con il computer acceso.
pub const SERVER_CHROME: &str = "chrome-devtools";

impl Motore {
    /// Con il computer dell'assistente acceso: `DISPLAY` e `XBOT_COMPUTER_HOME` nell'ambiente
    /// del turno (non `HOME`: i client tengono lì il loro accesso), per Claude Code il server MCP
    /// `chrome-devtools` collegato al Chrome del suo schermo, e due righe nel testo di sistema.
    pub(super) fn computer_nel_turno(&self, l: &mut Lancio) {
        let Some((display, casa, porta)) = self.computer.per_turno(&l.assistente_id) else {
            return;
        };
        l.richiesta.ambiente.push((
            "DISPLAY".into(),
            xbot_core::SegretoTesto::nuovo(display.clone()),
        ));
        l.richiesta.ambiente.push((
            "XBOT_COMPUTER_HOME".into(),
            xbot_core::SegretoTesto::nuovo(casa.to_string_lossy().into_owned()),
        ));
        let url = format!("http://127.0.0.1:{porta}");
        let mut testo = format!(
            "Hai un computer tuo: uno schermo ({display}, {}x{}) con Chrome aperto, e l'utente ti \
             vede dalla scheda Computer. Chrome ha il debug su {url}.",
            crate::computer::LARGHEZZA,
            crate::computer::ALTEZZA
        );
        if let Some(cfg) = l.richiesta.mcp.as_mut() {
            let gia: Vec<String> = cfg.server.iter().map(|s| s.nome().to_string()).collect();
            let nome = nome_mcp(SERVER_CHROME, &gia);
            cfg.server.push(mcp::ServerMcp::Stdio {
                nome: nome.clone(),
                comando: "npx".into(),
                argomenti: vec![
                    "-y".into(),
                    "chrome-devtools-mcp@latest".into(),
                    "--browserUrl".into(),
                    url,
                ],
                ambiente: Vec::new(),
            });
            testo.push_str(&format!(
                " Per usarlo hai gli strumenti del server MCP «{nome}»: quello che fai si vede \
                 sul tuo schermo."
            ));
        }
        l.richiesta.sistema = Some(match l.richiesta.sistema.take() {
            Some(s) => format!("{s}\n\n---\n\n{testo}"),
            None => testo,
        });
    }
}
// --- fine 2.H ---

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn nomi_dei_collegamenti() {
        let mut gia = Vec::new();
        for (nome, atteso) in [
            ("Odoo Ristorante", "odoo_ristorante"),
            ("xbot", "c_xbot"),
            ("Odoo Ristorante", "odoo_ristorante_2"),
            ("!!", "c_"),
        ] {
            let n = nome_mcp(nome, &gia);
            assert_eq!(n, atteso);
            gia.push(n);
        }
    }
}
