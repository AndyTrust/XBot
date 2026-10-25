//! La parte sincrona del motore: invio e riga d'eco, scelta del provider, turno e richiesta.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use xbot_core::{Codice, Errore, GenereRiga, Risposta, Ruolo};
use xbot_db::invii::{self, EsitoCrea, Invio, NuovoInvio};
use xbot_db::turni::{self, NuovoTurno, Turno};
use xbot_db::{
    Connessione, assistenti, controlli, impostazioni, lettura, provider, righe, sessioni,
};
use xbot_provider::RichiestaTurno;

use super::DatiInvio;

/// Tutto quello che serve per lanciare un turno già scritto nel database.
pub(super) struct Lancio {
    pub invio_id: String,
    pub assistente_id: String,
    pub turno: Turno,
    pub richiesta: RichiestaTurno,
    pub file_sistema: Vec<PathBuf>,
    // --- S-B (2.E) ---
    /// La macchina collegata che esegue il turno (`None` = qui sul server).
    pub macchina_remota: Option<String>,
    /// «Auto» con il computer spento: il turno gira qui, con un avviso.
    pub ripiego: Option<String>,
    /// Il turno vuole Chrome del Mac (`assistente.usa_chrome`).
    pub chrome: bool,
    // --- fine S-B ---
}

// --- S-B (2.E) ---
/// Cosa c'è adesso: provider caricati qui, macchine collegate (socket aperto), id del server.
#[derive(Debug, Clone, Default)]
pub struct Disponibili {
    pub ids: Vec<String>,
    pub connesse: Vec<String>,
    pub server: String,
}

/// La scelta di un turno: provider, modello, dove gira.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Scelta {
    pub provider: String,
    pub modello: Option<String>,
    pub macchina_remota: Option<String>,
    /// Il nome della macchina spenta da cui «Auto» è ripiegato qui.
    pub ripiego: Option<String>,
}

/// La macchina remota voluta, se ha un lavoratore (provider dichiarati) e non è il server.
fn remota_voluta(
    c: &Connessione,
    voluta: Option<&str>,
    server: &str,
) -> Risposta<Option<(String, Vec<String>)>> {
    let Some(m) = voluta.filter(|m| !m.is_empty() && *m != server) else {
        return Ok(None);
    };
    let prov = provider::della_macchina(c, m)?;
    Ok((!prov.is_empty()).then(|| (m.to_string(), prov)))
}
// --- fine S-B ---

pub(super) enum Preparato {
    Duplicato(Box<(Invio, Option<Turno>)>),
    Rifiutato(Invio),
    Pronto {
        invio: Invio,
        eco_id: String,
        eco_seq: i64,
    },
}

/// Crea l'invio e la riga d'eco, oppure dice duplicato o rifiutato.
pub(super) fn prepara_invio(
    c: &Connessione,
    sessione_id: &str,
    d: &DatiInvio,
    disp: &Disponibili,
) -> Risposta<Preparato> {
    let sessione = sessioni::leggi(c, sessione_id)?;
    controlli::assistente_attivo(c, &sessione.assistente_id)?;
    controlli::riferimento(c, "provider", d.provider.as_deref())?;
    controlli::riferimento(c, "macchina", d.macchina.as_deref())?;
    let nuovo = NuovoInvio {
        id: d.id.clone(),
        sessione_id: sessione_id.to_string(),
        testo: d.testo.clone(),
        allegati: d.allegati.clone(),
        risposta_a_riga_id: d.risposta_a.clone(),
        origine: d.origine.clone(),
        macchina_destinazione_id: d.macchina.clone(),
        provider_richiesto_id: d.provider.clone(),
        composto_ms: d.composto_ms,
    };
    let invio = match invii::crea(c, &nuovo)? {
        EsitoCrea::Duplicato(i) => {
            let t = turni::per_invio(c, &i.id)?;
            return Ok(Preparato::Duplicato(Box::new((i, t))));
        }
        EsitoCrea::Nuovo(i) => i,
    };
    let verifica = if sessione.stato != "attiva" {
        Err(Errore::nuovo(
            Codice::SessioneArchiviata,
            "la sessione è archiviata",
        ))
    } else {
        scegli_provider(
            c,
            &sessione,
            d.provider.as_deref(),
            d.macchina.as_deref(),
            disp,
        )
        .map(|_| ())
    };
    if let Err(e) = verifica {
        return Ok(Preparato::Rifiutato(invii::rifiuta(c, &invio.id, &e)?));
    }
    let corpo = json!({
        "testo": d.testo, "allegati": d.allegati, "risposta_a": d.risposta_a, "invio_id": d.id,
        // --- S2 --- l'origine (menzione:, collega:, canale:…) per la cornice in chat.
        "origine": d.origine,
    });
    // --- S2 --- il testo di un altro assistente ha il ruolo `collega`.
    let ruolo = if d.origine.starts_with("collega:") || d.origine.starts_with("figlia:") {
        Ruolo::Collega
    } else {
        Ruolo::Utente
    };
    // --- fine S2 ---
    let eco = righe::aggiungi(c, sessione_id, GenereRiga::Messaggio, Some(ruolo), &corpo)?;
    Ok(Preparato::Pronto {
        invio,
        eco_id: eco.id,
        eco_seq: eco.aggiornata_seq,
    })
}

/// Provider, modello e macchina per un turno. «auto» = il primo provider attivo (della macchina
/// collegata voluta, o del server). Mai ripiego silenzioso sul modello: se quello scelto manca,
/// `modello_non_disponibile`. (S-B 2.E) Un provider che sta solo su un computer collegato va
/// su quel computer; se il computer è spento il turno aspetta, salvo «auto», che ripiega qui.
pub(super) fn scegli_provider(
    c: &Connessione,
    s: &sessioni::Sessione,
    richiesto: Option<&str>,
    macchina_richiesta: Option<&str>,
    disp: &Disponibili,
) -> Risposta<Scelta> {
    let a = assistenti::leggi(c, &s.assistente_id)?;
    let dell_assistente = a["provider_predefinito_id"].as_str().map(str::to_string);
    let richiesto = richiesto.filter(|r| !r.is_empty());
    let scelto = richiesto
        .map(str::to_string)
        .or_else(|| s.provider_id.clone())
        .or(dell_assistente)
        .unwrap_or_else(|| "auto".into());
    let fermo = || {
        Errore::nuovo(
            Codice::ProviderFermo,
            format!("il provider {scelto} non è attivo"),
        )
    };
    let voluta = macchina_richiesta
        .filter(|m| !m.is_empty())
        .map(str::to_string)
        .or_else(|| s.macchina_id.clone());
    let remota = remota_voluta(c, voluta.as_deref(), &disp.server)?;
    let locale_ok = |p: &str| -> Risposta<bool> {
        Ok(disp.ids.iter().any(|i| i == p) && provider::attivo(c, p)?)
    };
    let mut ripiego = None;
    let (prov, macchina_remota) = if scelto == "auto" {
        match remota {
            Some((m, prov)) if disp.connesse.contains(&m) => (prov[0].clone(), Some(m)),
            Some((m, _)) => {
                let nome = xbot_db::macchine::leggi(c, &m)?["nome"]
                    .as_str()
                    .unwrap_or("il computer")
                    .to_string();
                ripiego = Some(nome);
                (
                    provider::primo_attivo(c, &disp.ids)?.ok_or_else(fermo)?,
                    None,
                )
            }
            None => (
                provider::primo_attivo(c, &disp.ids)?.ok_or_else(fermo)?,
                None,
            ),
        }
    } else {
        if !provider::attivo(c, &scelto)? {
            return Err(fermo());
        }
        match remota {
            Some((m, prov)) if prov.contains(&scelto) => (scelto.clone(), Some(m)),
            Some(_) if locale_ok(&scelto)? => (scelto.clone(), None),
            Some(_) => {
                return Err(Errore::nuovo(
                    Codice::ProviderFermo,
                    format!("il provider {scelto} non è su quel computer"),
                ));
            }
            None if locale_ok(&scelto)? => (scelto.clone(), None),
            None => {
                let dove = provider::macchine_del_provider(c, &scelto)?;
                let m = dove
                    .iter()
                    .find(|m| disp.connesse.contains(m))
                    .or(dove.first())
                    .cloned()
                    .ok_or_else(fermo)?;
                (scelto.clone(), Some(m))
            }
        }
    };
    // Il modello della sessione vale solo per il provider della sessione.
    let stesso =
        richiesto.is_none() || s.provider_id.is_none() || richiesto == s.provider_id.as_deref();
    let mut modello = if stesso { s.modello_id.clone() } else { None };
    if let Some(m) = &modello {
        let verifica = provider::verifica_modello(c, &prov, m);
        match verifica {
            // Col ripiego dichiarato il modello del computer spento non vale qui.
            Err(_) if ripiego.is_some() => modello = None,
            altro => altro?,
        }
    }
    Ok(Scelta {
        provider: prov,
        modello,
        macchina_remota,
        ripiego,
    })
}

/// Scrive il turno e costruisce la richiesta per il provider.
pub(super) fn prepara_turno(
    c: &Connessione,
    invio: &Invio,
    disp: &Disponibili,
) -> Risposta<Lancio> {
    let s = sessioni::leggi(c, &invio.sessione_id)?;
    let scelta = scegli_provider(
        c,
        &s,
        invio.provider_richiesto_id.as_deref(),
        invio.macchina_destinazione_id.as_deref(),
        disp,
    )?;
    let (prov, modello) = (scelta.provider.clone(), scelta.modello.clone());
    let macchina_turno = match (&scelta.macchina_remota, &scelta.ripiego) {
        (Some(m), _) => Some(m.clone()),
        (None, Some(_)) => Some(disp.server.clone()).filter(|m| !m.is_empty()),
        (None, None) => invio
            .macchina_destinazione_id
            .clone()
            .or(s.macchina_id.clone()),
    };
    let turno = turni::crea(
        c,
        &NuovoTurno {
            sessione_id: s.id.clone(),
            invio_id: Some(invio.id.clone()),
            provider_id: prov.clone(),
            modello_id: modello.clone(),
            livello: s.livello_ragionamento.clone(),
            macchina_id: macchina_turno,
        },
    )?;
    lettura::imposta_lavoro(c, &s.assistente_id, true, false)?;
    let a = assistenti::leggi(c, &s.assistente_id)?;
    let mut file_sistema = Vec::new();
    if let Some(p) = a["profilo_percorso"].as_str() {
        file_sistema.push(PathBuf::from(p));
    }
    if let Some(Value::String(p)) = impostazioni::leggi(c, "memoria.ingresso")? {
        file_sistema.push(PathBuf::from(p));
    }
    if let Some(cartella) = a["memoria_cartella"].as_str() {
        let cartella = Path::new(cartella);
        // La nota del progetto: `<cartella>/<nome>.md`, o `Memoria.md` se quella non c'è.
        if let Some(nome) = cartella.file_name() {
            let mut nota = nome.to_os_string();
            nota.push(".md");
            let nota = cartella.join(nota);
            let alternativa = cartella.join("Memoria.md");
            file_sistema.push(if !nota.is_file() && alternativa.is_file() {
                alternativa
            } else {
                nota
            });
        }
    }
    // Col ripiego la sessione del client stava sull'altro computer: qui si riparte.
    let stessa_esterna =
        scelta.ripiego.is_none() && s.provider_id.as_deref().is_none_or(|p| p == prov);
    // Un modello salvato come `<provider>/<id>` (stesso id di un altro provider) va al client
    // senza il prefisso.
    let modello_client = modello
        .as_ref()
        .map(|m| m.strip_prefix(&format!("{prov}/")).unwrap_or(m).to_string());
    let chrome = a["usa_chrome"].as_i64() == Some(1);
    let allegati = invio
        .allegati
        .as_array()
        .map(|v| {
            v.iter()
                .filter_map(|a| a.as_str().map(PathBuf::from))
                .collect()
        })
        .unwrap_or_default();
    Ok(Lancio {
        invio_id: invio.id.clone(),
        assistente_id: s.assistente_id.clone(),
        turno,
        richiesta: RichiestaTurno {
            sessione_id: s.id.clone(),
            sessione_esterna_id: if stessa_esterna {
                s.sessione_esterna_id.clone()
            } else {
                None
            },
            modello: modello_client,
            livello: s.livello_ragionamento.clone(),
            cartella_lavoro: s.cartella_lavoro.as_ref().map(PathBuf::from),
            testo: invio.testo.clone(),
            allegati,
            sistema: None,
            // Lo schema v2 non ha ancora un modo dei permessi per sessione: vale quello
            // predefinito dell'adattatore (`acceptEdits`).
            permessi: None,
            // --- S2 --- configurazione MCP e ambiente: li riempie il motore al lancio.
            mcp: None,
            ambiente: Vec::new(),
            // --- 2.G --- i permessi ACP di grok: li collega il motore al lancio.
            chiedi_permesso: None,
        },
        file_sistema,
        macchina_remota: scelta.macchina_remota,
        ripiego: scelta.ripiego,
        chrome,
    })
}

/// Profilo e memoria: i file che esistono, uniti. Quelli mancanti si saltano con un avviso nel log.
pub(super) async fn testo_di_sistema(file: &[PathBuf]) -> Option<String> {
    let mut parti = Vec::new();
    for f in file {
        match tokio::fs::read_to_string(f).await {
            Ok(t) if !t.trim().is_empty() => parti.push(t),
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(file = %f.display(), errore = %e, "file di sistema non letto");
            }
        }
    }
    (!parti.is_empty()).then(|| parti.join("\n\n---\n\n"))
}
