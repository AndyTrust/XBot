//! L'errore di XBot: un codice testuale stabile più un testo per le persone.

use serde::Serialize;

/// Codici stabili: l'interfaccia li confronta, quindi non si rinominano.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Codice {
    NonAutorizzato,
    AccessoNegato,
    TroppiTentativi,
    /// Password giusta, codice TOTP mancante su un dispositivo non fidato.
    TotpRichiesto,
    NonTrovato,
    SessioneNonTrovata,
    AssistenteNonTrovato,
    AssistenteArchiviato,
    ProviderNonTrovato,
    SezioneNonTrovata,
    RiferimentoNonTrovato,
    TurnoNonTrovato,
    VoceNonTrovata,
    ModelloNonDisponibile,
    ProviderFermo,
    InvioDuplicato,
    CursoreVecchio,
    TurnoInCorso,
    SessioneArchiviata,
    RichiestaNonValida,
    TestoTroppoLungo,
    CorpoTroppoGrande,
    ChiaveRiservata,
    CassaforteNonImpostata,
    PassphraseSbagliata,
    Cifratura,
    Database,
    Interno,
    // --- S1 (Fase 2.D) ---
    CassaforteChiusa,
    CartaNonValida,
    RichiestaNonTrovata,
    RichiestaGiaRisolta,
    RoutineNonTrovata,
    MacchinaNonTrovata,
    MacchinaNonConnessa,
    TipoNonAccettato,
    PushNonConfigurato,
    // --- fine S1 ---
    // --- S2 (Fase 2.D) ---
    CreditoVoceFinito,
    VoceNonConfigurata,
    VoceFallita,
    TestoNonValido,
    RichiestaChiusa,
    CanaleRevocato,
    VariabileNonAmmessa,
    TroppeChiamate,
    AudioNonValido,
    // --- fine S2 ---
    // --- 2.I ---
    OauthStatoNonValido,
    OauthFallito,
    // --- E (aggiornamenti) ---
    AggiornamentiDisattivati,
    SorgenteSconosciuta,
    InCorso,
}

impl Codice {
    pub fn come_testo(self) -> &'static str {
        match self {
            Self::NonAutorizzato => "non_autorizzato",
            Self::AccessoNegato => "accesso_negato",
            Self::TroppiTentativi => "troppi_tentativi",
            Self::TotpRichiesto => "totp_richiesto",
            Self::NonTrovato => "non_trovato",
            Self::SessioneNonTrovata => "sessione_non_trovata",
            Self::AssistenteNonTrovato => "assistente_non_trovato",
            Self::AssistenteArchiviato => "assistente_archiviato",
            Self::ProviderNonTrovato => "provider_non_trovato",
            Self::SezioneNonTrovata => "sezione_non_trovata",
            Self::RiferimentoNonTrovato => "riferimento_non_trovato",
            Self::TurnoNonTrovato => "turno_non_trovato",
            Self::VoceNonTrovata => "voce_non_trovata",
            Self::ModelloNonDisponibile => "modello_non_disponibile",
            Self::ProviderFermo => "provider_fermo",
            Self::InvioDuplicato => "invio_duplicato",
            Self::CursoreVecchio => "cursore_vecchio",
            Self::TurnoInCorso => "turno_in_corso",
            Self::SessioneArchiviata => "sessione_archiviata",
            Self::RichiestaNonValida => "richiesta_non_valida",
            Self::TestoTroppoLungo => "testo_troppo_lungo",
            Self::CorpoTroppoGrande => "corpo_troppo_grande",
            Self::ChiaveRiservata => "chiave_riservata",
            Self::CassaforteNonImpostata => "cassaforte_non_impostata",
            Self::PassphraseSbagliata => "passphrase_sbagliata",
            Self::Cifratura => "cifratura",
            Self::Database => "database",
            Self::Interno => "interno",
            // --- S1 (Fase 2.D) ---
            Self::CassaforteChiusa => "cassaforte_chiusa",
            Self::CartaNonValida => "carta_non_valida",
            Self::RichiestaNonTrovata => "richiesta_non_trovata",
            Self::RichiestaGiaRisolta => "richiesta_gia_risolta",
            Self::RoutineNonTrovata => "routine_non_trovata",
            Self::MacchinaNonTrovata => "macchina_non_trovata",
            Self::MacchinaNonConnessa => "macchina_non_connessa",
            Self::TipoNonAccettato => "tipo_non_accettato",
            Self::PushNonConfigurato => "push_non_configurato",
            // --- fine S1 ---
            // --- S2 (Fase 2.D) ---
            Self::CreditoVoceFinito => "credito_voce_finito",
            Self::VoceNonConfigurata => "voce_non_configurata",
            Self::VoceFallita => "voce_fallita",
            Self::TestoNonValido => "testo_non_valido",
            Self::RichiestaChiusa => "richiesta_chiusa",
            Self::CanaleRevocato => "canale_revocato",
            Self::VariabileNonAmmessa => "variabile_non_ammessa",
            Self::TroppeChiamate => "troppe_chiamate",
            Self::AudioNonValido => "audio_non_valido",
            Self::OauthStatoNonValido => "oauth_stato_non_valido",
            Self::OauthFallito => "oauth_fallito",
            Self::AggiornamentiDisattivati => "aggiornamenti_disattivati",
            Self::SorgenteSconosciuta => "sorgente_sconosciuta",
            Self::InCorso => "in_corso",
            // --- fine S2 ---
        }
    }
}

/// L'errore che attraversa i crate.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}: {testo}", codice.come_testo())]
pub struct Errore {
    pub codice: Codice,
    pub testo: String,
}

impl Errore {
    pub fn nuovo(codice: Codice, testo: impl Into<String>) -> Self {
        Self {
            codice,
            testo: testo.into(),
        }
    }

    pub fn non_valida(testo: impl Into<String>) -> Self {
        Self::nuovo(Codice::RichiestaNonValida, testo)
    }

    pub fn interno(testo: impl Into<String>) -> Self {
        Self::nuovo(Codice::Interno, testo)
    }

    pub fn database(testo: impl Into<String>) -> Self {
        Self::nuovo(Codice::Database, testo)
    }
}

/// Il risultato di ogni operazione di XBot.
pub type Risposta<T> = Result<T, Errore>;
