//! Il motore dei turni: riceve gli invii, sceglie il provider, avvia il turno, scrive le righe.
//! Un solo turno per sessione: gli invii che arrivano durante un turno vanno in coda.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use xbot_core::{Codice, Consegna, Errore, Risposta, StatoInvio, StatoTurno};
use xbot_db::Db;
use xbot_db::invii::{self, Invio};
use xbot_db::turni::{self, Conto, Turno};

use crate::bus::{Bus, Segnale};
use crate::registro::Registro;
use crate::traduttore::{Fine, Traduttore};

mod avvisi;
mod chiusura;
// --- S2 ---
mod mcp;
pub use mcp::{nome_mcp, testo_del_turno};
// --- fine S2 ---
mod preparazione;

use preparazione::{Lancio, Preparato, prepara_invio, prepara_turno, testo_di_sistema};
// --- S-B (2.E) --- turni sui computer collegati.
mod lavori;
pub use preparazione::Disponibili;
// --- fine S-B ---

/// Un invio come arriva dall'API.
#[derive(Debug, Clone)]
pub struct DatiInvio {
    pub id: String,
    pub testo: String,
    pub allegati: Value,
    pub risposta_a: Option<String>,
    pub provider: Option<String>,
    pub macchina: Option<String>,
    pub origine: String,
    pub composto_ms: Option<i64>,
}

/// La risposta a un invio: `{stato, consegna, riga_eco_id, turno_id}` più l'eventuale rifiuto.
#[derive(Debug, Clone, Serialize)]
pub struct EsitoInvio {
    pub invio_id: String,
    pub stato: String,
    pub consegna: Option<String>,
    pub riga_eco_id: Option<String>,
    pub turno_id: Option<String>,
    pub rifiuto: Option<Value>,
}

impl EsitoInvio {
    fn da(i: &Invio, stato: &str, turno_id: Option<String>) -> Self {
        Self {
            invio_id: i.id.clone(),
            stato: stato.to_string(),
            consegna: i.consegna.clone(),
            riga_eco_id: i.riga_eco_id.clone(),
            turno_id,
            rifiuto: i
                .rifiuto_codice
                .as_ref()
                .map(|c| json!({ "codice": c, "testo": i.rifiuto_testo })),
        }
    }
}

#[derive(Default)]
struct Attivo {
    turno_id: Option<String>,
    interrompi: Option<Box<dyn FnOnce() + Send>>,
    interrotto: bool,
    coda: VecDeque<String>,
}

#[derive(Clone)]
pub struct Motore {
    db: Db,
    registro: Registro,
    bus: Bus,
    attivi: Arc<Mutex<HashMap<String, Attivo>>>,
    // --- S1 ---
    /// La cartella dei profili e delle abilità (per il testo di sistema).
    profili: Arc<std::path::PathBuf>,
    // --- fine S1 ---
    // --- S2 ---
    /// Socket MCP, turni con MCP, chiave della cassaforte, limiti delle menzioni.
    s2: Arc<crate::mcp_interno::StatoS2>,
    // --- fine S2 ---
    // --- S-B (2.E) ---
    /// I computer collegati (canale WebSocket) e i loro lavori.
    macchine: crate::canale_macchina::Hub,
    // --- fine S-B ---
    // --- 2.H --- i computer degli assistenti (desktop sulla VPS).
    computer: crate::computer::Computer,
    // --- fine 2.H ---
}

impl Motore {
    pub fn nuovo(db: Db, registro: Registro, bus: Bus) -> Self {
        Self {
            db: db.clone(),
            registro,
            bus,
            attivi: Arc::default(),
            profili: Arc::new(std::path::PathBuf::from("profili")),
            s2: Arc::new(crate::mcp_interno::StatoS2::temporaneo()),
            macchine: crate::canale_macchina::Hub::default(),
            computer: crate::computer::Computer::nuovo(
                db.clone(),
                std::env::temp_dir().join(format!("xbot-computer-{}", xbot_core::nuovo_id())),
            ),
        }
    }

    // --- 2.H ---
    /// I computer degli assistenti.
    pub fn computer(&self) -> &crate::computer::Computer {
        &self.computer
    }
    // --- fine 2.H ---

    // --- S1 ---
    /// La cartella dei profili (`XBOT_PROFILI`).
    pub fn con_profili(mut self, profili: std::path::PathBuf) -> Self {
        self.profili = Arc::new(profili);
        self
    }
    // --- fine S1 ---

    // --- S2 ---
    /// Lo stato di S2 (nella cartella dei dati, con la chiave della cassaforte).
    pub fn con_s2(mut self, s2: crate::mcp_interno::StatoS2) -> Self {
        self.s2 = Arc::new(s2);
        self
    }
    // --- fine S2 ---

    fn attivi(&self) -> MutexGuard<'_, HashMap<String, Attivo>> {
        match self.attivi.lock() {
            Ok(g) => g,
            Err(avvelenato) => avvelenato.into_inner(),
        }
    }

    /// Vero se la sessione ha un turno in corso o in coda.
    pub fn occupata(&self, sessione_id: &str) -> bool {
        self.attivi().contains_key(sessione_id)
    }

    /// Riceve un invio: idempotente per id; parte subito o va in coda.
    pub async fn invia(&self, sessione_id: &str, d: DatiInvio) -> Risposta<EsitoInvio> {
        if d.id.trim().is_empty() || d.id.len() > 64 {
            return Err(Errore::non_valida(
                "l'id dell'invio vuole da 1 a 64 caratteri",
            ));
        }
        if d.testo.trim().is_empty() {
            return Err(Errore::non_valida("il testo è vuoto"));
        }
        xbot_db::controlli::lunghezza("testo", Some(&d.testo), xbot_db::controlli::TESTO_INVIO)?;
        // --- S2 --- le menzioni dell'utente si inoltrano dopo che il suo invio è stato accettato.
        let menzioni_dell_utente = crate::menzioni::origine_dell_utente(&d.origine)
            && !crate::menzioni::trova(&d.testo).is_empty();
        let testo_per_menzioni = menzioni_dell_utente.then(|| d.testo.clone());
        // --- fine S2 ---
        let (sid, disp) = (sessione_id.to_string(), self.disponibili());
        let prep = self
            .db
            .esegui(move |c| prepara_invio(c, &sid, &d, &disp))
            .await?;
        let (invio, eco_id, eco_seq) = match prep {
            Preparato::Duplicato(d) => {
                let (i, t) = *d;
                return Ok(EsitoInvio::da(&i, "duplicato", t.map(|t| t.id)));
            }
            Preparato::Rifiutato(i) => return Ok(EsitoInvio::da(&i, "rifiutato", None)),
            Preparato::Pronto {
                invio,
                eco_id,
                eco_seq,
            } => (invio, eco_id, eco_seq),
        };
        self.bus.pubblica(
            sessione_id,
            Segnale::Righe {
                aggiornata_seq: eco_seq,
            },
        );
        // --- S2 ---
        if let Some(testo) = testo_per_menzioni {
            let (m, sid) = (self.clone(), sessione_id.to_string());
            tokio::spawn(async move { crate::menzioni::da_utente(&m, &sid, &testo).await });
        }
        // --- fine S2 ---
        let parte_ora = {
            let mut attivi = self.attivi();
            match attivi.get_mut(sessione_id) {
                Some(a) => {
                    a.coda.push_back(invio.id.clone());
                    false
                }
                None => {
                    attivi.insert(sessione_id.to_string(), Attivo::default());
                    true
                }
            }
        };
        let iid = invio.id.clone();
        if !parte_ora {
            let i = self
                .db
                .esegui(move |c| {
                    invii::accetta(c, &iid, Consegna::InCodaProssimoTurno, Some(&eco_id))
                })
                .await?;
            return Ok(EsitoInvio::da(&i, "accettato", None));
        }
        let disp = self.disponibili();
        let (i, lancio) = self
            .db
            .esegui(move |c| {
                let i = invii::accetta(c, &iid, Consegna::AvviatoContinuazione, Some(&eco_id))?;
                let l = prepara_turno(c, &i, &disp);
                Ok((i, l))
            })
            .await?;
        let motore = self.clone();
        let sid = sessione_id.to_string();
        match lancio {
            Ok(lancio) => {
                let turno_id = lancio.turno.id.clone();
                tokio::spawn(async move { motore.ciclo(&sid, Some(lancio)).await });
                Ok(EsitoInvio::da(&i, "accettato", Some(turno_id)))
            }
            Err(e) => {
                self.segna_fallito(sessione_id, &i.id, &e).await;
                tokio::spawn(async move { motore.ciclo(&sid, None).await });
                let mut esito = EsitoInvio::da(&i, "fallito", None);
                esito.rifiuto = Some(json!({ "codice": e.codice, "testo": e.testo }));
                Ok(esito)
            }
        }
    }

    /// Esegue il turno dato, poi quelli in coda, finché la coda è vuota.
    async fn ciclo(&self, sessione_id: &str, mut lancio: Option<Lancio>) {
        loop {
            let prossimo = match lancio.take() {
                Some(l) => self.esegui_turno(sessione_id, l).await,
                None => self.prossimo_o_libera(sessione_id),
            };
            lancio = self.prepara_prossimo(sessione_id, prossimo).await;
            if lancio.is_none() {
                return;
            }
        }
    }

    /// Prende il prossimo invio in coda; se non c'è libera la sessione. Tutto sotto un lucchetto,
    /// così un invio che arriva ora o trova la sessione occupata (e va in coda) o la trova libera.
    fn prossimo_o_libera(&self, sessione_id: &str) -> Option<String> {
        let mut attivi = self.attivi();
        let a = attivi.get_mut(sessione_id)?;
        match a.coda.pop_front() {
            Some(i) => {
                a.turno_id = None;
                a.interrompi = None;
                a.interrotto = false;
                Some(i)
            }
            None => {
                attivi.remove(sessione_id);
                None
            }
        }
    }

    /// Scrive il turno per l'invio dato; se non può partire passa al successivo.
    async fn prepara_prossimo(
        &self,
        sessione_id: &str,
        mut invio: Option<String>,
    ) -> Option<Lancio> {
        loop {
            let invio_id = invio.take()?;
            let disp = self.disponibili();
            let iid = invio_id.clone();
            let r = self
                .db
                .esegui(move |c| prepara_turno(c, &invii::leggi(c, &iid)?, &disp))
                .await;
            match r {
                Ok(l) => return Some(l),
                Err(e) => {
                    self.segna_fallito(sessione_id, &invio_id, &e).await;
                    invio = self.prossimo_o_libera(sessione_id);
                }
            }
        }
    }

    /// Un invio che non può partire: riga `avviso` e stato `fallito`.
    async fn segna_fallito(&self, sessione_id: &str, invio_id: &str, e: &Errore) {
        let (sid, iid, e2) = (sessione_id.to_string(), invio_id.to_string(), e.clone());
        let r = self
            .db
            .esegui(move |c| {
                invii::imposta_stato(c, &iid, StatoInvio::Fallito)?;
                let corpo = json!({ "codice": e2.codice, "testo": e2.testo, "invio_id": iid });
                xbot_db::righe::aggiungi(
                    c,
                    &sid,
                    xbot_core::GenereRiga::Avviso,
                    Some(xbot_core::Ruolo::Sistema),
                    &corpo,
                )
            })
            .await;
        match r {
            Ok(riga) => self.bus.pubblica(
                sessione_id,
                Segnale::Righe {
                    aggiornata_seq: riga.aggiornata_seq,
                },
            ),
            Err(e) => tracing::error!(errore = %e, "invio fallito non registrato"),
        }
    }

    /// Esegue un turno; restituisce il prossimo invio in coda (o `None`: sessione libera).
    async fn esegui_turno(&self, sessione_id: &str, mut l: Lancio) -> Option<String> {
        let turno_id = l.turno.id.clone();
        let gia_interrotto = {
            let mut attivi = self.attivi();
            let a = attivi.entry(sessione_id.to_string()).or_default();
            a.turno_id = Some(turno_id.clone());
            a.interrotto
        };
        self.bus.pubblica(
            sessione_id,
            Segnale::Turno {
                turno_id: turno_id.clone(),
                stato: StatoTurno::InCorso.come_testo().into(),
            },
        );
        crate::mcp_interno::figlie::sincronizza(&self.db, &self.bus, sessione_id, false).await;
        // --- S1 --- testo di sistema (profilo, abilità, istruzioni fisse, memoria) e allegati.
        let file_memoria =
            crate::sistema::file_di_memoria(&self.db, &l.assistente_id, &l.file_sistema).await;
        let memoria = testo_di_sistema(&file_memoria).await;
        l.richiesta.sistema =
            crate::sistema::componi(&self.db, &self.profili, sessione_id, memoria).await;
        l.richiesta.allegati =
            crate::sistema::allegati_del_turno(&self.db, sessione_id, &l.invio_id).await;
        // --- fine S1 ---
        tracing::info!(
            turno = %turno_id,
            provider = %l.turno.provider_id,
            caratteri = l.richiesta.testo.chars().count(),
            "turno avviato"
        );
        let mut trad = Traduttore::nuovo(
            self.db.clone(),
            self.bus.clone(),
            sessione_id,
            &turno_id,
            &l.turno.provider_id,
        );
        // --- S-B (2.E) --- «Auto» con il computer spento: si dice che il turno gira qui.
        if let Some(nome) = &l.ripiego {
            let testo = format!(
                "{nome} spento: eseguo sulla VPS con {}",
                l.turno.provider_id
            );
            if let Err(e) = trad.avviso_errore("ripiego_vps", &testo).await {
                tracing::error!(errore = %e, "avviso del ripiego non scritto");
            }
        }
        let remota = l.macchina_remota.clone();
        // --- fine S-B ---
        // --- S2 --- configurazione MCP del turno (server interno, collegamenti, segreti).
        if remota.is_none() {
            self.s2_prepara(&mut l).await;
            // --- 2.H --- il computer dell'assistente: DISPLAY, Chrome via MCP, testo di sistema.
            self.computer_nel_turno(&mut l);
        }
        // --- fine S2 ---
        let (fine, partito) = if gia_interrotto {
            (None, false)
        } else if let Some(m) = remota {
            self.parla_col_computer(sessione_id, &l, &mut trad, &m)
                .await
        } else {
            self.parla_col_provider(sessione_id, &l, &mut trad).await
        };
        let prossimo = self
            .chiudi_turno(sessione_id, &l, &trad, fine, partito)
            .await;
        // --- S2 --- pulizia del turno e menzioni nel testo dell'assistente.
        self.s2_dopo_turno(sessione_id, &l).await;
        // --- fine S2 ---
        prossimo
    }

    /// Parla col provider. Il booleano dice se il provider è partito davvero.
    async fn parla_col_provider(
        &self,
        sessione_id: &str,
        l: &Lancio,
        trad: &mut Traduttore,
    ) -> (Option<Fine>, bool) {
        let Some(prov) = self.registro.trova(&l.turno.provider_id) else {
            let fine = Fine::Fallito {
                codice: Codice::ProviderFermo.come_testo().into(),
                testo: "provider non caricato".into(),
            };
            return (Some(fine), false);
        };
        let (tx, mut rx) = mpsc::channel(64);
        let maniglia = match prov.avvia(l.richiesta.clone(), tx).await {
            Ok(m) => m,
            Err(e) => {
                if let Err(e2) = trad.avviso_errore(e.codice.come_testo(), &e.testo).await {
                    tracing::error!(errore = %e2, "avviso non scritto");
                }
                let fine = Fine::Fallito {
                    codice: e.codice.come_testo().into(),
                    testo: e.testo,
                };
                return (Some(fine), false);
            }
        };
        if let Some(pid) = maniglia.pid {
            let tid = l.turno.id.clone();
            if let Err(e) = self
                .db
                .esegui(move |c| turni::imposta_pid(c, &tid, pid))
                .await
            {
                tracing::warn!(errore = %e, "pid non salvato");
            }
        }
        let da_fermare = {
            let mut attivi = self.attivi();
            let a = attivi.entry(sessione_id.to_string()).or_default();
            if a.interrotto {
                Some(maniglia.interrompi)
            } else {
                a.interrompi = Some(maniglia.interrompi);
                None
            }
        };
        if let Some(f) = da_fermare {
            f();
        }
        while let Some(ev) = rx.recv().await {
            match trad.applica(ev).await {
                Ok(Some(fine)) => return (Some(fine), true),
                Ok(None) => {}
                Err(e) => {
                    tracing::error!(errore = %e, "evento del turno non scritto");
                    let fine = Fine::Fallito {
                        codice: e.codice.come_testo().into(),
                        testo: e.testo,
                    };
                    return (Some(fine), true);
                }
            }
        }
        (None, true)
    }

    /// Ferma un turno in corso. Su un turno già chiuso restituisce lo stato com'è.
    pub async fn interrompi(&self, turno_id: &str) -> Risposta<Turno> {
        let tid = turno_id.to_string();
        let turno = self.db.esegui(move |c| turni::leggi(c, &tid)).await?;
        if turno.stato != StatoTurno::InCorso.come_testo() {
            return Ok(turno);
        }
        let fermo = {
            let mut attivi = self.attivi();
            match attivi.get_mut(&turno.sessione_id) {
                Some(a) if a.turno_id.as_deref().is_none_or(|t| t == turno_id) => {
                    a.interrotto = true;
                    a.interrompi.take()
                }
                _ => None,
            }
        };
        if let Some(f) = fermo {
            f();
        }
        let tid = turno_id.to_string();
        let t = self
            .db
            .esegui(move |c| {
                turni::chiudi(c, &tid, StatoTurno::Interrotto, &Conto::default(), None)?;
                turni::leggi(c, &tid)
            })
            .await?;
        tracing::info!(turno = %turno_id, "turno interrotto");
        self.bus.pubblica(
            &t.sessione_id,
            Segnale::Turno {
                turno_id: t.id.clone(),
                stato: t.stato.clone(),
            },
        );
        Ok(t)
    }
}
