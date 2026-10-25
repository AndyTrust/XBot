//! I servizi della Fase 2.D (S1) nello stato del server: chiave della cassaforte, cartelle dei
//! dati (profili, blob, allegati), attese delle richieste, chiavi VAPID, macchina del server,
//! e i compiti in sottofondo (battito, orologio delle routine, notifiche push).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use xbot_core::{Codice, Errore, Ms, Risposta, SegretoTesto, ora_ms};
use xbot_db::cassaforte::{self, Chiave};
use xbot_db::{Db, impostazioni, macchine};

use crate::push::Vapid;
use crate::richieste::Attese;

/// Dove sta il server: id stabile, nome e genere della riga `macchina`.
#[derive(Debug, Clone)]
pub struct MacchinaServer {
    pub id: String,
    pub nome: String,
    pub genere: String,
}

pub struct Servizi {
    /// `None` = cassaforte chiusa (manca `XBOT_CASSAFORTE_CHIAVE` o è sbagliata): 503.
    pub cassaforte: Option<Arc<Chiave>>,
    pub cartella_dati: PathBuf,
    /// `XBOT_PROFILI`: profili e abilità degli assistenti.
    pub profili: PathBuf,
    pub blob: PathBuf,
    pub richieste: Attese,
    pub vapid: Option<Arc<Vapid>>,
    /// `XBOT_URL_PUBBLICO`: per il comando del biglietto e il contatto VAPID.
    pub url_pubblico: Option<String>,
    pub macchina: MacchinaServer,
    pub avviato_ms: Ms,
}

impl Servizi {
    /// La chiave della cassaforte, o 503 `cassaforte_chiusa`.
    pub fn chiave(&self) -> Risposta<Arc<Chiave>> {
        self.cassaforte.clone().ok_or_else(|| {
            Errore::nuovo(
                Codice::CassaforteChiusa,
                "la cassaforte è chiusa: manca XBOT_CASSAFORTE_CHIAVE sul server",
            )
        })
    }
}

/// Le opzioni da cui nascono i servizi.
#[derive(Default)]
pub struct OpzioniServizi {
    pub cartella_dati: PathBuf,
    pub profili: Option<PathBuf>,
    pub chiave_cassaforte: Option<SegretoTesto>,
    pub vapid_privata: Option<SegretoTesto>,
    pub vapid_pubblica: Option<String>,
    pub url_pubblico: Option<String>,
    pub nome_macchina: Option<String>,
}

impl OpzioniServizi {
    /// Dall'ambiente: `XBOT_DATI` (predefinita la cartella di `XBOT_DB`),
    /// `XBOT_PROFILI`, `XBOT_CASSAFORTE_CHIAVE`, `XBOT_VAPID_PRIVATA`,
    /// `XBOT_VAPID_PUBBLICA`, `XBOT_URL_PUBBLICO`, `XBOT_MACCHINA_NOME`.
    pub fn da_ambiente(db: &str) -> Self {
        let leggi = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let cartella_dati = leggi("XBOT_DATI").map(PathBuf::from).unwrap_or_else(|| {
            Path::new(db)
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
        });
        Self {
            cartella_dati,
            profili: leggi("XBOT_PROFILI").map(PathBuf::from),
            chiave_cassaforte: leggi("XBOT_CASSAFORTE_CHIAVE").map(SegretoTesto::from),
            vapid_privata: leggi("XBOT_VAPID_PRIVATA").map(SegretoTesto::from),
            vapid_pubblica: leggi("XBOT_VAPID_PUBBLICA"),
            url_pubblico: leggi("XBOT_URL_PUBBLICO"),
            nome_macchina: leggi("XBOT_MACCHINA_NOME"),
        }
    }

    /// Per le prove: una cartella temporanea nuova, cassaforte aperta solo se si dà la frase.
    pub fn per_prove(chiave: Option<&str>) -> Self {
        let dir = std::env::temp_dir().join(format!("xbot-prova-{}", xbot_core::nuovo_id()));
        Self {
            cartella_dati: dir,
            chiave_cassaforte: chiave.map(SegretoTesto::nuovo),
            ..Default::default()
        }
    }
}

/// Apre la cassaforte con la frase: la prima volta imposta sale e verifica, poi la controlla.
fn apri_cassaforte(db: &Db, frase: &SegretoTesto) -> Option<Arc<Chiave>> {
    let r = db.con(|c| match cassaforte::apri(c, frase) {
        Err(e) if e.codice == Codice::CassaforteNonImpostata => {
            cassaforte::imposta_passphrase(c, frase)
        }
        altro => altro,
    });
    match r {
        Ok(k) => Some(Arc::new(k)),
        Err(e) => {
            tracing::error!(
                codice = e.codice.come_testo(),
                "cassaforte non aperta: resta chiusa"
            );
            None
        }
    }
}

/// L'id stabile della macchina del server (`server.macchina_id` nelle impostazioni).
fn id_macchina(db: &Db) -> Risposta<String> {
    db.con(|c| {
        if let Some(serde_json::Value::String(id)) = impostazioni::leggi(c, "server.macchina_id")? {
            return Ok(id);
        }
        let id = xbot_core::nuovo_id();
        impostazioni::scrivi(c, "server.macchina_id", &serde_json::json!(id))?;
        Ok(id)
    })
}

/// Prepara i servizi (bloccante: Argon2 e database). Registra il server come macchina.
pub fn prepara(db: &Db, o: OpzioniServizi) -> Risposta<Servizi> {
    let crea = |p: &Path| {
        std::fs::create_dir_all(p).map_err(|e| Errore::interno(format!("{}: {e}", p.display())))
    };
    let profili = o
        .profili
        .clone()
        .unwrap_or_else(|| o.cartella_dati.join("profili"));
    let blob = o.cartella_dati.join("blob");
    for p in [&o.cartella_dati, &profili, &blob] {
        crea(p)?;
    }
    let cassaforte = o
        .chiave_cassaforte
        .as_ref()
        .and_then(|f| apri_cassaforte(db, f));
    if cassaforte.is_none() && o.chiave_cassaforte.is_none() {
        tracing::warn!("XBOT_CASSAFORTE_CHIAVE mancante: cassaforte chiusa");
    }
    let vapid = match (&o.vapid_privata, &o.vapid_pubblica) {
        (Some(p), _) => match Vapid::da_privata(p) {
            Ok(v) => Some(Arc::new(v)),
            Err(e) => {
                tracing::error!(errore = %e.testo, "chiave VAPID non valida: push spente");
                None
            }
        },
        _ => None,
    };
    let genere = if cfg!(target_os = "macos") {
        "mac"
    } else {
        "vps"
    };
    let macchina = MacchinaServer {
        id: id_macchina(db)?,
        nome: o.nome_macchina.clone().unwrap_or_else(|| {
            if genere == "mac" {
                "locale".into()
            } else {
                "vps".into()
            }
        }),
        genere: genere.into(),
    };
    db.con(|c| {
        macchine::registra_se_stessa(
            c,
            &macchina.id,
            &macchina.nome,
            &macchina.genere,
            std::env::consts::OS,
            std::env::consts::ARCH,
        )
    })?;
    Ok(Servizi {
        cassaforte,
        cartella_dati: o.cartella_dati,
        profili,
        blob,
        richieste: Attese::default(),
        vapid,
        url_pubblico: o.url_pubblico,
        macchina,
        avviato_ms: ora_ms(),
    })
}

/// I compiti in sottofondo: battito ogni 30 s, orologio delle routine ogni secondo,
/// notifiche push ogni 3 s. Si avviano da `servi`, non nelle prove.
pub fn avvia_compiti(stato: &crate::Stato) {
    let s = stato.clone();
    tokio::spawn(async move {
        let mut t = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            t.tick().await;
            let id = s.servizi.macchina.id.clone();
            if let Err(e) = s.db.esegui(move |c| macchine::battito(c, &id)).await {
                tracing::warn!(errore = %e, "battito non scritto");
            }
        }
    });
    crate::orologio::avvia(stato.clone());
    crate::push::avvia_notificatore(stato.clone());
    crate::quote_scadute::avvia(stato.clone());
    // --- 2.H --- i computer che l'utente vuole accesi ripartono.
    let c = stato.motore.computer().clone();
    tokio::spawn(async move { c.riavvia_accesi().await });
}
