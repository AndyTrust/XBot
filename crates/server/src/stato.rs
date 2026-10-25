//! Lo stato condiviso del server: database, provider, bus, motore e chiavi.

use std::sync::Arc;

use axum::extract::FromRef;
use axum_extra::extract::cookie::Key;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use xbot_core::{Errore, Risposta, SegretoTesto};
use xbot_db::cassaforte::{self, Chiave};
use xbot_db::{Db, impostazioni, turni};
use xbot_provider::Provider;

use crate::bus::Bus;
use crate::cliente::Rete;
use crate::limite::Limite;
use crate::motore::Motore;
use crate::registro::Registro;

/// Dove si salva il segreto del server se non arriva da `XBOT_SEGRETO_COOKIE`.
const CHIAVE_SEGRETO: &str = "server.segreto_cookie";
/// Etichetta storica della cifratura del seme TOTP (dal primo nome del progetto): fa parte della
/// chiave, quindi non si cambia, o i secondi fattori già impiantati non si leggono più.
const SALE_TOTP: &[u8] = b"plancia.totp.v1.sale";

pub struct Interno {
    pub db: Db,
    pub registro: Registro,
    pub bus: Bus,
    pub motore: Motore,
    pub chiave_cookie: Key,
    pub chiave_totp: Chiave,
    pub cookie_sicuro: bool,
    /// I proxy da cui si accetta `X-Forwarded-For` (`XBOT_PROXY_FIDATO`).
    pub proxy_fidati: Vec<Rete>,
    /// Tentativi di accesso falliti (5 ogni 5 minuti per indirizzo, 20 in tutto).
    pub tentativi: Limite,
    /// Diventa `true` alla chiusura del server: i flussi SSE mandano `chiusura` e finiscono.
    pub chiusura: tokio::sync::watch::Sender<bool>,
    // --- S1 ---
    /// Cassaforte, cartelle dei dati, attese delle richieste, VAPID, macchina del server.
    pub servizi: crate::servizi::Servizi,
    // --- fine S1 ---
    // --- 2.I ---
    /// I tentativi OAuth aperti (`state` → collegamento e verificatore PKCE).
    pub oauth_tentativi: crate::oauth::Tentativi,
    /// Gli accessi dei client in corso («Entra» dei provider).
    pub accessi: crate::accessi::Accessi,
    // --- fine 2.I ---
}

impl Interno {
    /// Chiude i flussi SSE aperti (alla chiusura del server).
    pub fn chiudi_flussi(&self) {
        self.chiusura.send_replace(true);
    }
}

#[derive(Clone)]
pub struct Stato(pub Arc<Interno>);

impl std::ops::Deref for Stato {
    type Target = Interno;
    fn deref(&self) -> &Interno {
        &self.0
    }
}

impl FromRef<Stato> for Key {
    fn from_ref(s: &Stato) -> Key {
        s.0.chiave_cookie.clone()
    }
}

/// Il segreto del server: dall'ambiente, oppure dal database, oppure nuovo (e salvato).
pub fn segreto_server(db: &Db, da_ambiente: Option<&SegretoTesto>) -> Risposta<Vec<u8>> {
    let decodifica = |t: &str| -> Risposta<Vec<u8>> {
        let b = STANDARD
            .decode(t.trim())
            .map_err(|_| Errore::non_valida("segreto del cookie: base64 non valido"))?;
        if b.len() < 32 {
            return Err(Errore::non_valida(
                "segreto del cookie: servono almeno 32 byte",
            ));
        }
        Ok(b)
    };
    if let Some(s) = da_ambiente {
        return decodifica(s.esponi());
    }
    db.con(|c| {
        if let Some(v) = impostazioni::leggi(c, CHIAVE_SEGRETO)?
            && let Some(t) = v.as_str()
        {
            return decodifica(t);
        }
        let nuovo: [u8; 32] = rand::random();
        impostazioni::scrivi(
            c,
            CHIAVE_SEGRETO,
            &serde_json::json!(STANDARD.encode(nuovo)),
        )?;
        tracing::warn!("segreto del cookie generato e salvato nel database");
        Ok(nuovo.to_vec())
    })
}

/// La chiave che cifra il segreto TOTP, derivata dal segreto del server con Argon2id.
pub fn chiave_totp(segreto: &[u8]) -> Risposta<Chiave> {
    cassaforte::deriva(
        &SegretoTesto::nuovo(STANDARD.encode(segreto)),
        SALE_TOTP,
        cassaforte::PARAMETRI,
    )
}

impl Stato {
    /// Prepara lo stato senza proxy fidati.
    pub async fn prepara(
        db: Db,
        provider: Vec<Arc<dyn Provider>>,
        segreto_cookie: Option<SegretoTesto>,
        cookie_sicuro: bool,
    ) -> Risposta<Stato> {
        Self::prepara_con(db, provider, segreto_cookie, cookie_sicuro, Vec::new()).await
    }

    /// Prepara lo stato: segreti, provider nel database, turni orfani chiusi. I servizi della
    /// 2.D nascono in una cartella temporanea con la cassaforte chiusa (per le prove).
    pub async fn prepara_con(
        db: Db,
        provider: Vec<Arc<dyn Provider>>,
        segreto_cookie: Option<SegretoTesto>,
        cookie_sicuro: bool,
        proxy_fidati: Vec<Rete>,
    ) -> Risposta<Stato> {
        let opzioni = crate::servizi::OpzioniServizi::per_prove(None);
        Self::prepara_servizi(
            db,
            provider,
            segreto_cookie,
            cookie_sicuro,
            proxy_fidati,
            opzioni,
        )
        .await
    }

    // --- S1 ---
    /// Come `prepara_con`, con i servizi della 2.D dalle opzioni date (`servi` le prende
    /// dall'ambiente, le prove da `OpzioniServizi::per_prove`).
    pub async fn prepara_servizi(
        db: Db,
        provider: Vec<Arc<dyn Provider>>,
        segreto_cookie: Option<SegretoTesto>,
        cookie_sicuro: bool,
        proxy_fidati: Vec<Rete>,
        opzioni: crate::servizi::OpzioniServizi,
    ) -> Risposta<Stato> {
        let db3 = db.clone();
        let servizi = tokio::task::spawn_blocking(move || crate::servizi::prepara(&db3, opzioni))
            .await
            .map_err(|e| Errore::interno(format!("preparazione interrotta: {e}")))??;
        // --- fine S1 ---
        let db2 = db.clone();
        let (segreto, chiave_totp) = tokio::task::spawn_blocking(move || {
            let s = segreto_server(&db2, segreto_cookie.as_ref())?;
            let k = chiave_totp(&s)?;
            Ok::<_, Errore>((s, k))
        })
        .await
        .map_err(|e| Errore::interno(format!("preparazione interrotta: {e}")))??;
        let registro = Registro::nuovo(provider);
        registro.sincronizza(&db).await?;
        let orfani = db.esegui(turni::chiudi_orfani).await?;
        if orfani > 0 {
            tracing::warn!(orfani, "turni rimasti in corso chiusi come interrotti");
        }
        let bus = Bus::default();
        let motore = Motore::nuovo(db.clone(), registro.clone(), bus.clone())
            // --- S1 ---
            .con_profili(servizi.profili.clone())
            // --- fine S1 ---
            // --- S2 ---
            .con_s2({
                let s2 = crate::mcp_interno::StatoS2::in_cartella(servizi.cartella_dati.clone());
                s2.imposta_chiave(servizi.cassaforte.clone());
                s2.imposta_attese(servizi.richieste.clone());
                s2
            });
        // --- fine S2 ---
        // --- 2.H --- i computer degli assistenti in `XBOT_DATI/computer/`.
        motore
            .computer()
            .imposta_cartella(servizi.cartella_dati.join("computer"));
        // --- fine 2.H ---
        let stato = Stato(Arc::new(Interno {
            db,
            registro,
            bus,
            motore,
            chiave_cookie: Key::derive_from(&segreto),
            chiave_totp,
            cookie_sicuro,
            proxy_fidati,
            tentativi: Limite::default(),
            chiusura: tokio::sync::watch::Sender::new(false),
            servizi,
            oauth_tentativi: Default::default(),
            accessi: Default::default(),
        }));
        // --- S-B (2.E) --- computer collegati: id del server, lavori orfani, sorvegliante.
        stato
            .motore
            .macchine()
            .imposta_server(&stato.servizi.macchina.id);
        stato.motore.macchine().imposta_stato(&stato);
        match stato.db.esegui(xbot_db::lavori::chiudi_orfani).await {
            Ok(n) if n > 0 => tracing::warn!(n, "lavori rimasti aperti annullati"),
            Ok(_) => {}
            Err(e) => tracing::warn!(errore = %e, "lavori orfani non chiusi"),
        }
        crate::canale_macchina::avvia_sorvegliante(&stato);
        // --- fine S-B ---
        // --- 2.I --- gli account dei client del server, all'avvio e ogni 5 minuti.
        crate::accessi::avvia_lettore(&stato);
        // --- S2 --- il server MCP interno (socket Unix) per i turni di Claude Code.
        if let Err(e) = crate::mcp_interno::avvia(stato.clone()).await {
            tracing::warn!(errore = %e, "server MCP interno non avviato: i turni partono senza");
        }
        // --- fine S2 ---
        Ok(stato)
    }
}
