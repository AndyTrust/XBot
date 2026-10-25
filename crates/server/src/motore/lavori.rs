//! I turni sui computer collegati (Fase 2.E S-B): il turno diventa un `lavoro` spedito sul
//! canale WebSocket della macchina; gli `evento` del lavoratore passano nello stesso traduttore
//! dei turni locali. Se la macchina è spenta il lavoro resta `in_coda` («in attesa del Mac»),
//! parte quando si ricollega, scade dopo 24 ore; dopo 5 minuti di attesa arriva una push.

use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;
use xbot_core::{GenereRiga, Ruolo, StatoLavoro, ora_ms};
use xbot_db::{cassaforte_voci, collegamenti, eventi, lavori, macchine};
use xbot_provider::{EventoTurno, claude_code};

use super::{Lancio, Motore};
use crate::bus::Segnale;
use crate::canale_macchina::{Hub, Vivo};
use crate::traduttore::{Fine, Traduttore};

fn coppie(v: &[(String, xbot_core::SegretoTesto)]) -> Value {
    Value::Array(
        v.iter()
            .map(|(k, s)| json!({ "nome": k, "valore": s.esponi() }))
            .collect(),
    )
}

/// I nomi delle variabili e delle intestazioni, senza valori (per il carico salvato).
fn soli_nomi(v: &Value) -> Value {
    Value::Array(
        v.as_array()
            .map(|a| a.iter().map(|x| x["nome"].clone()).collect())
            .unwrap_or_default(),
    )
}

/// Il tetto degli allegati mandati nel frame `lavoro`.
pub const MAX_ALLEGATI_FILO: u64 = 25 * 1024 * 1024;

fn tipo_mime(nome: &str) -> &'static str {
    let est = nome.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    match est.as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("pdf") => "application/pdf",
        Some("txt" | "md" | "log") => "text/plain",
        Some("csv") => "text/csv",
        Some("json") => "application/json",
        Some("html" | "htm") => "text/html",
        Some("zip") => "application/zip",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("mp3") => "audio/mpeg",
        Some("m4a") => "audio/mp4",
        Some("webm") => "audio/webm",
        _ => "application/octet-stream",
    }
}

/// `allegati[{nome, tipo_mime, dimensione, contenuto_base64}]` se in tutto stanno entro
/// `MAX_ALLEGATI_FILO`; altrimenti `[]` e in coda al testo `[allegato rimasto sul server: …]`.
pub async fn allegati_sul_filo(percorsi: &[std::path::PathBuf], testo: &mut String) -> Value {
    use base64::Engine;
    let nome = |p: &std::path::Path| {
        p.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let mut totale = 0u64;
    for p in percorsi {
        totale += tokio::fs::metadata(p)
            .await
            .map(|m| m.len())
            .unwrap_or(u64::MAX / 4);
    }
    let mut v = Vec::new();
    if totale <= MAX_ALLEGATI_FILO {
        for p in percorsi {
            match tokio::fs::read(p).await {
                Ok(b) => {
                    let n = nome(p);
                    v.push(json!({
                        "nome": n, "tipo_mime": tipo_mime(&n), "dimensione": b.len(),
                        "contenuto_base64": base64::engine::general_purpose::STANDARD.encode(&b),
                    }));
                }
                Err(e) => {
                    tracing::warn!(errore = %e, "allegato non letto");
                    v.clear();
                    break;
                }
            }
        }
        if v.len() == percorsi.len() {
            return Value::Array(v);
        }
    }
    for p in percorsi {
        testo.push_str(&format!("\n[allegato rimasto sul server: {}]", nome(p)));
    }
    json!([])
}

impl Motore {
    /// I computer collegati.
    pub fn macchine(&self) -> &Hub {
        &self.macchine
    }

    /// Provider caricati qui, macchine con il canale aperto, id del server.
    pub fn disponibili(&self) -> super::Disponibili {
        super::Disponibili {
            ids: self.registro.ids(),
            connesse: self.macchine.connesse(),
            server: self.macchine.server_id(),
        }
    }

    /// La richiesta del turno sul filo (con i segreti, solo in memoria) e il carico da salvare
    /// (senza i valori dei segreti).
    async fn richiesta_sul_filo(&self, l: &Lancio, macchina_id: &str) -> (Value, Value) {
        let r = &l.richiesta;
        let claude = l.turno.provider_id == claude_code::ID;
        let riceve = xbot_provider::riceve_collegamenti(&l.turno.provider_id);
        if riceve {
            // --- 2.I e 2.K --- rinnovo OAuth e avviso dei collegamenti di altre macchine.
            self.s2k_prepara(l, macchina_id).await;
        }
        let chiave = self.s2.chiave();
        let (aid, sid, mid) = (
            l.assistente_id.clone(),
            l.turno.sessione_id.clone(),
            macchina_id.to_string(),
        );
        let letto = self
            .db
            .esegui(move |c| {
                let permesso = macchine::leggi(c, &mid)?["permesso_esecuzione"]
                    .as_str()
                    .unwrap_or("ask")
                    .to_string();
                let coll = if riceve {
                    collegamenti::per_assistente_su(c, chiave.as_deref(), &aid, &mid, false)?
                } else {
                    Vec::new()
                };
                let segreti = match chiave.as_deref() {
                    Some(k) => cassaforte_voci::segreti_per_assistente(c, k, &aid, Some(&sid))?,
                    None => Vec::new(),
                };
                Ok((permesso, coll, segreti))
            })
            .await;
        let (permesso, coll, segreti) = letto.unwrap_or_else(|e| {
            tracing::warn!(errore = %e, "collegamenti e segreti del lavoro non letti");
            ("ask".into(), Vec::new(), Vec::new())
        });
        // 2.L-bis: il nome sul filo è quello pulito e unico del `--mcp-config` del server
        // (`Posta (IMAP)` → `posta_imap`): Cursor rifiuta gli spazi e le parentesi, e il server
        // ritrova il collegamento dal nome dello strumento (`mcp__posta_imap__caselle`).
        let mut nomi: Vec<String> = Vec::new();
        let collegamenti: Vec<Value> = coll
            .into_iter()
            .map(|c| {
                let nome = super::mcp::nome_mcp(&c.nome, &nomi);
                nomi.push(nome.clone());
                json!({
                    "nome": nome, "trasporto": c.trasporto, "url": c.url,
                    "intestazioni": coppie(&c.intestazioni), "comando": c.comando,
                    "argomenti": c.argomenti, "ambiente": coppie(&c.ambiente),
                })
            })
            .collect();
        // Gli allegati viaggiano nel frame (fino a 25 MB in tutto); oltre, al computer arriva
        // solo il nome.
        let mut testo = r.testo.clone();
        let allegati = allegati_sul_filo(&r.allegati, &mut testo).await;
        let filo = json!({
            "provider_id": l.turno.provider_id,
            "sessione_esterna_id": r.sessione_esterna_id,
            "modello": r.modello,
            "livello": r.livello,
            "cartella_lavoro": r.cartella_lavoro.as_ref().map(|p| p.to_string_lossy()),
            "testo": testo,
            "allegati": allegati,
            "sistema": r.sistema,
            // Per cursor_agent, codex, gemini e grok il modo dei permessi è quello della macchina.
            "permessi": if claude { r.permessi.clone() } else { Some(permesso.clone()) },
            "ambiente": coppie(&r.ambiente.iter().cloned().chain(segreti).collect::<Vec<_>>()),
            "collegamenti": collegamenti,
            "chiedi_permessi": permesso != "always",
            "permesso_esecuzione": permesso,
            "chrome": l.chrome,
        });
        let mut salvato = filo.clone();
        salvato["ambiente"] = soli_nomi(&filo["ambiente"]);
        if let Some(cs) = salvato["collegamenti"].as_array_mut() {
            for c in cs {
                c["intestazioni"] = soli_nomi(&c["intestazioni"]);
                c["ambiente"] = soli_nomi(&c["ambiente"]);
            }
        }
        if let Some(al) = salvato["allegati"].as_array_mut() {
            for a in al {
                if let Some(o) = a.as_object_mut() {
                    o.remove("contenuto_base64");
                }
            }
        }
        salvato["sistema"] = json!(r.sistema.as_ref().map(|s| s.chars().count()));
        (filo, salvato)
    }

    pub(super) async fn riga_sistema(
        &self,
        sessione_id: &str,
        genere: GenereRiga,
        corpo: Value,
    ) -> Option<String> {
        let sid = sessione_id.to_string();
        match self
            .db
            .esegui(move |c| {
                xbot_db::righe::aggiungi(c, &sid, genere, Some(Ruolo::Sistema), &corpo)
            })
            .await
        {
            Ok(r) => {
                self.bus.pubblica(
                    sessione_id,
                    Segnale::Righe {
                        aggiornata_seq: r.aggiornata_seq,
                    },
                );
                Some(r.id)
            }
            Err(e) => {
                tracing::error!(errore = %e, "riga del lavoro non scritta");
                None
            }
        }
    }

    /// La riga «in attesa del Mac» chiusa: `risolto: true`, `stato: ripartito|scaduto`.
    async fn chiudi_attesa(
        &self,
        sessione_id: &str,
        riga: &mut Option<(String, Value)>,
        stato: &str,
    ) {
        let Some((id, mut corpo)) = riga.take() else {
            return;
        };
        corpo["risolto"] = json!(true);
        corpo["stato"] = json!(stato);
        match self
            .db
            .esegui(move |c| xbot_db::righe::riscrivi(c, &id, &corpo))
            .await
        {
            Ok(r) => self.bus.pubblica(
                sessione_id,
                Segnale::Righe {
                    aggiornata_seq: r.aggiornata_seq,
                },
            ),
            Err(e) => tracing::warn!(errore = %e, "riga dell'attesa non chiusa"),
        }
    }

    /// Il turno su un computer collegato. Il booleano dice se il turno è partito (lavoro creato).
    pub(super) async fn parla_col_computer(
        &self,
        sessione_id: &str,
        l: &Lancio,
        trad: &mut Traduttore,
        macchina_id: &str,
    ) -> (Option<Fine>, bool) {
        let (filo, salvato) = self.richiesta_sul_filo(l, macchina_id).await;
        let (mid, sid, tid) = (
            macchina_id.to_string(),
            sessione_id.to_string(),
            l.turno.id.clone(),
        );
        let creato = self
            .db
            .esegui(move |c| {
                let lc = lavori::crea(
                    c,
                    &lavori::NuovoLavoro {
                        macchina_id: &mid,
                        sessione_id: &sid,
                        turno_id: &tid,
                        carico: &salvato,
                    },
                )?;
                let nome = macchine::leggi(c, &mid)?["nome"]
                    .as_str()
                    .unwrap_or("computer")
                    .to_string();
                Ok((lc, nome))
            })
            .await;
        let (lc, nome) = match creato {
            Ok(x) => x,
            Err(e) => {
                return (
                    Some(Fine::Fallito {
                        codice: e.codice.come_testo().into(),
                        testo: e.testo,
                    }),
                    false,
                );
            }
        };
        self.s2l_nota_permessi(sessione_id, l).await;
        let frame = json!({
            "tipo": "lavoro", "id": lc.id, "generazione": lc.generazione,
            "recinto_id": lc.recinto_id, "sessione_id": sessione_id, "turno_id": l.turno.id,
            "richiesta": filo, "chrome": l.chrome,
        })
        .to_string();
        let (tx, mut rx) = mpsc::channel::<EventoTurno>(256);
        let ripiega = std::sync::Arc::new(tokio::sync::Notify::new());
        self.macchine.registra(
            &lc.id,
            Vivo {
                macchina_id: macchina_id.to_string(),
                sessione_id: sessione_id.to_string(),
                turno_id: l.turno.id.clone(),
                frame,
                preso: false,
                eventi: tx,
                sulla_vps: ripiega.clone(),
            },
        );
        // Interrompere = frame `interrompi` e fine del flusso degli eventi.
        let da_fermare = {
            let hub = self.macchine.clone();
            let lid = lc.id.clone();
            let ferma: Box<dyn FnOnce() + Send> = Box::new(move || hub.interrompi(&lid));
            let mut attivi = self.attivi();
            let a = attivi.entry(sessione_id.to_string()).or_default();
            if a.interrotto {
                Some(ferma)
            } else {
                a.interrompi = Some(ferma);
                None
            }
        };
        if let Some(f) = da_fermare {
            f();
        }
        let inviato = self.macchine.spedisci(&lc.id);
        let inizio = ora_ms();
        let tempi = self.macchine.tempi();
        let mut riga_attesa: Option<(String, Value)> = None;
        if !inviato {
            let corpo = json!({
                "codice": "in_attesa_macchina",
                "testo": format!("in attesa del {nome}: il turno parte quando si ricollega"),
                "turno_id": l.turno.id, "lavoro_id": lc.id, "macchina_id": macchina_id,
                "macchina": nome, "risolto": false, "stato": "in_attesa",
            });
            riga_attesa = self
                .riga_sistema(sessione_id, GenereRiga::Avviso, corpo.clone())
                .await
                .map(|id| (id, corpo));
            self.evento_sistema(
                "lavoro_in_attesa",
                json!({ "lavoro_id": lc.id, "macchina_id": macchina_id, "sessione_id": sessione_id }),
            )
            .await;
        }
        let mut avvisato = false;
        let mut sulla_vps = false;
        let fine = loop {
            let preso = self.macchine.preso(&lc.id);
            if preso && riga_attesa.is_some() {
                self.chiudi_attesa(sessione_id, &mut riga_attesa, "ripartito")
                    .await;
            }
            let attesa = if preso {
                Duration::from_secs(3600)
            } else {
                let trascorso = ora_ms() - inizio;
                let prossimo = if avvisato {
                    tempi.attesa_max_ms
                } else {
                    tempi.push_dopo_ms.min(tempi.attesa_max_ms)
                };
                Duration::from_millis(u64::try_from((prossimo - trascorso).max(10)).unwrap_or(10))
            };
            tokio::select! {
                () = ripiega.notified(), if !preso => {
                    sulla_vps = true;
                    break None;
                }
                ev = rx.recv() => match ev {
                    None => break None,
                    Some(ev) => match trad.applica(ev).await {
                        Ok(Some(f)) => break Some(f),
                        Ok(None) => {}
                        Err(e) => {
                            tracing::error!(errore = %e, "evento del lavoro non scritto");
                            break Some(Fine::Fallito { codice: e.codice.come_testo().into(), testo: e.testo });
                        }
                    },
                },
                () = tokio::time::sleep(attesa) => {
                    if self.macchine.preso(&lc.id) {
                        continue;
                    }
                    let trascorso = ora_ms() - inizio;
                    if trascorso >= tempi.attesa_max_ms {
                        let lid = lc.id.clone();
                        let _ = self
                            .db
                            .esegui(move |c| lavori::segna(c, &lid, StatoLavoro::Scaduto, None))
                            .await;
                        self.chiudi_attesa(sessione_id, &mut riga_attesa, "scaduto").await;
                        let testo = format!("il {nome} non si è ricollegato in tempo: lavoro scaduto");
                        if let Err(e) = trad.avviso_errore("lavoro_scaduto", &testo).await {
                            tracing::error!(errore = %e, "avviso non scritto");
                        }
                        break Some(Fine::Fallito { codice: "lavoro_scaduto".into(), testo });
                    }
                    if !avvisato && trascorso >= tempi.push_dopo_ms {
                        avvisato = true;
                        self.push_attesa(sessione_id, l, &nome).await;
                    }
                }
            }
        };
        self.macchine.togli(&lc.id);
        if riga_attesa.is_some() && !sulla_vps {
            let lid = lc.id.clone();
            let preso = self
                .db
                .esegui(move |c| Ok(lavori::leggi(c, &lid)?["preso_ms"].is_i64()))
                .await
                .unwrap_or(false);
            if preso {
                self.chiudi_attesa(sessione_id, &mut riga_attesa, "ripartito")
                    .await;
            }
        }
        if sulla_vps {
            return self
                .riparti_qui(sessione_id, l, trad, &lc.id, &mut riga_attesa)
                .await;
        }
        let stato = match &fine {
            Some(Fine::Finito { .. }) => StatoLavoro::Riuscito,
            Some(Fine::Fallito { .. }) => StatoLavoro::Fallito,
            None => StatoLavoro::Annullato,
        };
        let lid = lc.id.clone();
        if let Err(e) = self
            .db
            .esegui(move |c| lavori::segna(c, &lid, stato, None))
            .await
        {
            tracing::warn!(errore = %e, "stato del lavoro non scritto");
        }
        (fine, true)
    }

    /// «Esegui sulla VPS»: il lavoro in attesa si annulla e il turno riparte qui con
    /// `claude_code` (o il primo provider del server, se `claude_code` non c'è).
    async fn riparti_qui(
        &self,
        sessione_id: &str,
        l: &Lancio,
        trad: &mut Traduttore,
        lavoro_id: &str,
        riga_attesa: &mut Option<(String, Value)>,
    ) -> (Option<Fine>, bool) {
        self.chiudi_attesa(sessione_id, riga_attesa, "ripartito")
            .await;
        let prov = if self.registro.trova(claude_code::ID).is_some() {
            claude_code::ID.to_string()
        } else {
            self.registro.ids().into_iter().next().unwrap_or_default()
        };
        let (lid, tid, p2, server) = (
            lavoro_id.to_string(),
            l.turno.id.clone(),
            prov.clone(),
            self.macchine.server_id(),
        );
        let r = self
            .db
            .esegui(move |c| {
                lavori::segna(c, &lid, StatoLavoro::Annullato, Some(&json!({ "ripartito": "vps" })))?;
                c.execute(
                    "UPDATE turno SET provider_id = ?2, modello_id = NULL, macchina_id = ?3 WHERE id = ?1",
                    rusqlite::params![tid, p2, server],
                )
                .map_err(|e| xbot_core::Errore::database(e.to_string()))?;
                xbot_db::turni::leggi(c, &tid)
            })
            .await;
        let turno = match r {
            Ok(t) => t,
            Err(e) => {
                return (
                    Some(Fine::Fallito {
                        codice: e.codice.come_testo().into(),
                        testo: e.testo,
                    }),
                    true,
                );
            }
        };
        let mut richiesta = l.richiesta.clone();
        richiesta.modello = None;
        richiesta.sessione_esterna_id = None;
        richiesta.mcp = None;
        let mut l2 = Lancio {
            invio_id: l.invio_id.clone(),
            assistente_id: l.assistente_id.clone(),
            turno,
            richiesta,
            file_sistema: l.file_sistema.clone(),
            macchina_remota: None,
            ripiego: None,
            chrome: false,
        };
        let testo = format!("eseguo sulla VPS con {prov}");
        if let Err(e) = trad.avviso_errore("ripiego_vps", &testo).await {
            tracing::error!(errore = %e, "avviso non scritto");
        }
        self.s2_prepara(&mut l2).await;
        self.parla_col_provider(sessione_id, &l2, trad).await
    }

    /// Una riga del diario di sistema (`evento_sistema`).
    pub async fn evento_sistema(&self, tipo: &'static str, dettaglio: Value) {
        if let Err(e) = self
            .db
            .esegui(move |c| eventi::scrivi(c, tipo, &dettaglio))
            .await
        {
            tracing::warn!(errore = %e, "evento di sistema non scritto");
        }
    }

    /// La push «tocca a te» quando un lavoro aspetta il computer da più di 5 minuti.
    async fn push_attesa(&self, sessione_id: &str, l: &Lancio, macchina: &str) {
        let Some(stato) = self.macchine.stato_server() else {
            return;
        };
        let aid = l.assistente_id.clone();
        let nome = self
            .db
            .esegui(move |c| {
                Ok(xbot_db::assistenti::leggi(c, &aid)?["nome"]
                    .as_str()
                    .unwrap_or("Assistente")
                    .to_string())
            })
            .await
            .unwrap_or_else(|_| "Assistente".into());
        let n = crate::push::Notifica {
            tipo: "tocca_a_te",
            titolo: format!("{nome}: tocca a te"),
            corpo: format!("Il turno aspetta il {macchina}: accendilo o esegui sulla VPS"),
            sessione_id: Some(sessione_id.to_string()),
            assistente_id: Some(l.assistente_id.clone()),
        };
        let cliente = reqwest::Client::new();
        if let Err(e) = crate::push::notifica(&stato, &cliente, n).await {
            tracing::warn!(errore = %e, "push dell'attesa non mandata");
        }
    }
}
