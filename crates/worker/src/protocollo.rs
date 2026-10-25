//! I frame del canale fra lavoratore e server (contratto 2.E, sezione 2): JSON su una riga,
//! con il campo `tipo`. Tutta la serializzazione sta qui, per allinearla in un posto solo ai
//! nomi che fissa S-B (sezione 8 del contratto).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use xbot_core::{Ms, SegretoTesto};
use xbot_provider::EventoTurno;

/// Un client trovato sul Mac, con i suoi modelli (in `enroll` e nel `battito`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderMacchina {
    pub id: String,
    pub nome: String,
    pub programma: String,
    #[serde(default)]
    pub versione: Option<String>,
    pub modelli: Vec<ModelloMacchina>,
    /// 2.I: l'account del client (`None` = non collegato).
    #[serde(default)]
    pub account: Option<xbot_provider::Account>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelloMacchina {
    pub id: String,
    pub etichetta: String,
    #[serde(default)]
    pub dettaglio: Option<String>,
    #[serde(default)]
    pub livelli: Vec<String>,
}

/// Un `EventoTurno` sul filo: `{"tipo": "testo_delta", "testo": "…"}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum EventoFilo {
    Avviato {
        sessione_esterna_id: Option<String>,
    },
    TestoDelta {
        testo: String,
    },
    TestoCompleto {
        testo: String,
    },
    Ragionamento {
        testo: String,
    },
    ChiamataStrumento {
        nome: String,
        input_json: Value,
    },
    EsitoStrumento {
        nome: String,
        esito_json: Value,
        ok: bool,
    },
    Avviso {
        testo: String,
    },
    Finito {
        token_ingresso: Option<i64>,
        token_uscita: Option<i64>,
        token_cache: Option<i64>,
        costo_usd: Option<f64>,
        sessione_esterna_id: Option<String>,
    },
    Quota {
        finestra: String,
        utilizzo: f64,
        azzera_ms: Option<Ms>,
    },
    Fallito {
        codice: String,
        testo: String,
    },
}

impl From<EventoTurno> for EventoFilo {
    fn from(e: EventoTurno) -> Self {
        use EventoTurno as E;
        match e {
            E::Avviato {
                sessione_esterna_id,
            } => Self::Avviato {
                sessione_esterna_id,
            },
            E::TestoDelta(testo) => Self::TestoDelta { testo },
            E::TestoCompleto(testo) => Self::TestoCompleto { testo },
            E::Ragionamento(testo) => Self::Ragionamento { testo },
            E::ChiamataStrumento { nome, input_json } => {
                Self::ChiamataStrumento { nome, input_json }
            }
            E::EsitoStrumento {
                nome,
                esito_json,
                ok,
            } => Self::EsitoStrumento {
                nome,
                esito_json,
                ok,
            },
            E::Avviso(testo) => Self::Avviso { testo },
            E::Finito {
                token_ingresso,
                token_uscita,
                token_cache,
                costo_usd,
                sessione_esterna_id,
            } => Self::Finito {
                token_ingresso,
                token_uscita,
                token_cache,
                costo_usd,
                sessione_esterna_id,
            },
            E::Quota {
                finestra,
                utilizzo,
                azzera_ms,
            } => Self::Quota {
                finestra,
                utilizzo,
                azzera_ms,
            },
            E::Fallito { codice, testo } => Self::Fallito { codice, testo },
        }
    }
}

impl From<EventoFilo> for EventoTurno {
    fn from(e: EventoFilo) -> Self {
        use EventoFilo as F;
        match e {
            F::Avviato {
                sessione_esterna_id,
            } => Self::Avviato {
                sessione_esterna_id,
            },
            F::TestoDelta { testo } => Self::TestoDelta(testo),
            F::TestoCompleto { testo } => Self::TestoCompleto(testo),
            F::Ragionamento { testo } => Self::Ragionamento(testo),
            F::ChiamataStrumento { nome, input_json } => {
                Self::ChiamataStrumento { nome, input_json }
            }
            F::EsitoStrumento {
                nome,
                esito_json,
                ok,
            } => Self::EsitoStrumento {
                nome,
                esito_json,
                ok,
            },
            F::Avviso { testo } => Self::Avviso(testo),
            F::Finito {
                token_ingresso,
                token_uscita,
                token_cache,
                costo_usd,
                sessione_esterna_id,
            } => Self::Finito {
                token_ingresso,
                token_uscita,
                token_cache,
                costo_usd,
                sessione_esterna_id,
            },
            F::Quota {
                finestra,
                utilizzo,
                azzera_ms,
            } => Self::Quota {
                finestra,
                utilizzo,
                azzera_ms,
            },
            F::Fallito { codice, testo } => Self::Fallito { codice, testo },
        }
    }
}

/// Lavoratore → server. `seq` cresce per lavoro (da 1): il server conferma con `ok`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum DalLavoratore {
    Battito {
        posti_liberi: u32,
        provider: Vec<ProviderMacchina>,
        /// I lavori ancora in corso qui (per il lease).
        #[serde(default)]
        lavori: Vec<String>,
    },
    Preso {
        lavoro_id: String,
        generazione: i64,
        seq: u64,
    },
    Evento {
        lavoro_id: String,
        seq: u64,
        evento: EventoFilo,
    },
    /// Una richiesta di permesso del `claude` locale; `richiesta_id` lo sceglie il lavoratore e
    /// torna in `risolta`.
    Richiesta {
        lavoro_id: String,
        seq: u64,
        richiesta_id: String,
        /// Sul filo `tipo_richiesta` (`tipo` è il nome del frame): `permesso_locale`, ….
        #[serde(rename = "tipo_richiesta")]
        tipo: String,
        carico: Value,
    },
    /// 2.I: com'è andato `accedi`/`esci`. `stato`: `in_attesa` (con `url`), `riuscito`,
    /// `fallito`, `scaduto`, `terminale` (con `comando`), `uscito`.
    Accesso {
        #[serde(default)]
        richiesta_id: Option<String>,
        provider_id: String,
        stato: String,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        codice_utente: Option<String>,
        #[serde(default)]
        chiede_codice: bool,
        #[serde(default)]
        account: Option<xbot_provider::Account>,
        #[serde(default)]
        messaggio: Option<String>,
        #[serde(default)]
        comando: Option<String>,
    },
    /// 2.K: l'esito di `prova_collegamento` (`initialize` + `tools/list` fatti qui).
    EsitoProva {
        richiesta_id: String,
        ok: bool,
        strumenti: Vec<xbot_provider::prova_mcp::StrumentoMcp>,
        #[serde(default)]
        errore: Option<String>,
    },
    /// `stato`: `riuscito` | `fallito` | `interrotto` | `rifiutato`.
    Chiuso {
        lavoro_id: String,
        seq: u64,
        stato: String,
        #[serde(default)]
        motivo: Option<String>,
    },
}

impl DalLavoratore {
    pub fn lavoro_seq(&self) -> Option<(&str, u64)> {
        match self {
            Self::Battito { .. } | Self::Accesso { .. } | Self::EsitoProva { .. } => None,
            Self::Preso { lavoro_id, seq, .. }
            | Self::Evento { lavoro_id, seq, .. }
            | Self::Richiesta { lavoro_id, seq, .. }
            | Self::Chiuso { lavoro_id, seq, .. } => Some((lavoro_id, *seq)),
        }
    }
}

/// Il corpo di un frame di lavoro, prima che il giornale gli dia il `seq`.
#[derive(Debug, Clone, PartialEq)]
pub enum Corpo {
    Preso {
        generazione: i64,
    },
    Evento(EventoFilo),
    Richiesta {
        richiesta_id: String,
        tipo: String,
        carico: Value,
    },
    Chiuso {
        stato: String,
        motivo: Option<String>,
    },
}

impl Corpo {
    pub fn in_frame(self, lavoro_id: &str, seq: u64) -> DalLavoratore {
        let lavoro_id = lavoro_id.to_string();
        match self {
            Corpo::Preso { generazione } => DalLavoratore::Preso {
                lavoro_id,
                generazione,
                seq,
            },
            Corpo::Evento(evento) => DalLavoratore::Evento {
                lavoro_id,
                seq,
                evento,
            },
            Corpo::Richiesta {
                richiesta_id,
                tipo,
                carico,
            } => DalLavoratore::Richiesta {
                lavoro_id,
                seq,
                richiesta_id,
                tipo,
                carico,
            },
            Corpo::Chiuso { stato, motivo } => DalLavoratore::Chiuso {
                lavoro_id,
                seq,
                stato,
                motivo,
            },
        }
    }
}

/// Una variabile o un'intestazione con il valore segreto (mai nel `Debug`).
#[derive(Debug, Clone, Deserialize)]
pub struct Coppia {
    pub nome: String,
    pub valore: SegretoTesto,
}

/// Un collegamento MCP dell'assistente, già decifrato dal server.
#[derive(Debug, Clone, Deserialize)]
pub struct CollegamentoFilo {
    pub nome: String,
    /// `http` | `stdio`
    #[serde(default)]
    pub trasporto: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub intestazioni: Vec<Coppia>,
    #[serde(default)]
    pub comando: Option<String>,
    #[serde(default)]
    pub argomenti: Vec<String>,
    #[serde(default)]
    pub ambiente: Vec<Coppia>,
}

/// Un allegato del turno.
#[derive(Clone, Deserialize)]
#[serde(untagged)]
pub enum AllegatoFilo {
    Contenuto {
        nome: String,
        #[serde(default)]
        tipo_mime: Option<String>,
        #[serde(default)]
        dimensione: Option<u64>,
        contenuto_base64: String,
    },
    Percorso(String),
}

impl std::fmt::Debug for AllegatoFilo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contenuto {
                nome,
                tipo_mime,
                contenuto_base64,
                ..
            } => write!(
                f,
                "Allegato({nome}, {tipo_mime:?}, {} caratteri base64)",
                contenuto_base64.len()
            ),
            Self::Percorso(p) => write!(f, "Allegato({p})"),
        }
    }
}

/// La `RichiestaTurno` sul filo: senza percorsi del server, con i segreti in memoria.
#[derive(Debug, Clone, Deserialize)]
pub struct RichiestaFilo {
    pub provider_id: String,
    #[serde(default)]
    pub sessione_esterna_id: Option<String>,
    #[serde(default)]
    pub modello: Option<String>,
    #[serde(default)]
    pub livello: Option<String>,
    /// Usata solo se esiste sul Mac; altrimenti `<sessioni>/<id>` (vedi `credenziale::cartella_sessioni`).
    #[serde(default)]
    pub cartella_lavoro: Option<String>,
    pub testo: String,
    /// Un percorso già sul Mac o il file intero (`{nome, tipo_mime, dimensione,
    /// contenuto_base64}`, fino a 25 MB in tutto), scritto in `turni/<id>/allegati/`.
    #[serde(default)]
    pub allegati: Vec<AllegatoFilo>,
    #[serde(default)]
    pub sistema: Option<String>,
    #[serde(default)]
    pub permessi: Option<String>,
    #[serde(default)]
    pub ambiente: Vec<Coppia>,
    #[serde(default)]
    pub collegamenti: Vec<CollegamentoFilo>,
    /// I permessi passano dall'utente (strumento `permesso` del lavoratore). Vero se manca.
    #[serde(default = "vero")]
    pub chiedi_permessi: bool,
    /// Il permesso della macchina: `always` | `never` | `ask` (2.L-quater: per `claude_code`
    /// decide il modo dei permessi; se manca vale `chiedi_permessi`).
    #[serde(default)]
    pub permesso_esecuzione: Option<String>,
    /// Il turno usa Chrome del Mac: niente `--strict-mcp-config`.
    #[serde(default)]
    pub chrome: bool,
}

fn vero() -> bool {
    true
}

/// Server → lavoratore.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum DalServer {
    Ciao {
        macchina_id: String,
        #[serde(default)]
        server_versione: Option<String>,
    },
    Lavoro {
        id: String,
        generazione: i64,
        #[serde(default)]
        recinto_id: Option<String>,
        sessione_id: String,
        #[serde(default)]
        turno_id: Option<String>,
        /// Anche `carico` (come `lavoro.carico_json` del server).
        #[serde(alias = "carico")]
        richiesta: Box<RichiestaFilo>,
        /// Ammesso anche fuori dalla richiesta.
        #[serde(default)]
        chrome: bool,
    },
    Interrompi {
        lavoro_id: String,
    },
    Risolta {
        richiesta_id: String,
        /// `approvata` | `negata` | `ritirata` | `scaduta`
        esito: String,
        #[serde(default)]
        comando_approvato: Option<String>,
        #[serde(default)]
        motivo: Option<String>,
    },
    /// 2.I: «Entra» di un client: lancia il suo comando di accesso e manda gli `accesso`.
    Accedi {
        richiesta_id: String,
        provider_id: String,
    },
    /// 2.I: «Esci» di un client.
    Esci {
        richiesta_id: String,
        provider_id: String,
    },
    /// 2.K: prova un collegamento che gira qui e risponde con `esito_prova`.
    ProvaCollegamento {
        richiesta_id: String,
        collegamento: Box<CollegamentoFilo>,
    },
    /// 2.I: il codice da incollare nel comando di accesso in corso (Claude Code).
    CodiceAccesso {
        provider_id: String,
        codice: String,
    },
    /// Conferma: il server ha tutto fino a `fino_a_seq` compreso.
    Ok {
        lavoro_id: String,
        fino_a_seq: u64,
    },
    #[serde(other)]
    Sconosciuto,
}

#[cfg(test)]
#[path = "protocollo_prove.rs"]
mod test;
