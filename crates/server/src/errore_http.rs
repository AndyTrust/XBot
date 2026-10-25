//! Errori in HTTP: `{ "errore": { "codice", "testo" } }` con lo stato giusto, ed estrattori
//! JSON e query che rispondono nello stesso formato.

use axum::Json;
use axum::extract::{FromRequest, FromRequestParts, Query, Request};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use xbot_core::{Codice, Errore};

/// Il corpo più grande accettato dall'API (2 MB).
pub const LIMITE_CORPO: usize = 2 * 1024 * 1024;

#[derive(Debug)]
pub struct ErroreHttp(pub Errore);

impl From<Errore> for ErroreHttp {
    fn from(e: Errore) -> Self {
        Self(e)
    }
}

pub type RisHttp<T> = Result<Json<T>, ErroreHttp>;

pub fn stato_http(c: Codice) -> StatusCode {
    match c {
        Codice::NonAutorizzato
        | Codice::AccessoNegato
        | Codice::TotpRichiesto
        | Codice::PassphraseSbagliata => StatusCode::UNAUTHORIZED,
        Codice::ChiaveRiservata => StatusCode::FORBIDDEN,
        Codice::TroppiTentativi => StatusCode::TOO_MANY_REQUESTS,
        Codice::NonTrovato
        | Codice::SessioneNonTrovata
        | Codice::AssistenteNonTrovato
        | Codice::ProviderNonTrovato
        | Codice::SezioneNonTrovata
        | Codice::TurnoNonTrovato
        | Codice::VoceNonTrovata => StatusCode::NOT_FOUND,
        Codice::InvioDuplicato
        | Codice::CursoreVecchio
        | Codice::TurnoInCorso
        | Codice::SessioneArchiviata
        | Codice::AssistenteArchiviato
        | Codice::ModelloNonDisponibile => StatusCode::CONFLICT,
        Codice::ProviderFermo => StatusCode::SERVICE_UNAVAILABLE,
        Codice::RichiestaNonValida
        | Codice::CassaforteNonImpostata
        | Codice::TestoTroppoLungo
        | Codice::RiferimentoNonTrovato => StatusCode::BAD_REQUEST,
        Codice::CorpoTroppoGrande => StatusCode::PAYLOAD_TOO_LARGE,
        Codice::Cifratura | Codice::Database | Codice::Interno => StatusCode::INTERNAL_SERVER_ERROR,
        // --- S1 (Fase 2.D) ---
        Codice::CassaforteChiusa | Codice::PushNonConfigurato => StatusCode::SERVICE_UNAVAILABLE,
        Codice::CartaNonValida => StatusCode::BAD_REQUEST,
        Codice::RichiestaNonTrovata | Codice::RoutineNonTrovata | Codice::MacchinaNonTrovata => {
            StatusCode::NOT_FOUND
        }
        Codice::RichiestaGiaRisolta | Codice::MacchinaNonConnessa => StatusCode::CONFLICT,
        Codice::TipoNonAccettato => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        // --- fine S1 ---
        // --- S2 (Fase 2.D) ---
        Codice::CreditoVoceFinito => StatusCode::PAYMENT_REQUIRED,
        Codice::VoceNonConfigurata => StatusCode::SERVICE_UNAVAILABLE,
        Codice::VoceFallita => StatusCode::BAD_GATEWAY,
        Codice::TestoNonValido | Codice::VariabileNonAmmessa | Codice::AudioNonValido => {
            StatusCode::BAD_REQUEST
        }
        Codice::RichiestaChiusa | Codice::CanaleRevocato => StatusCode::CONFLICT,
        Codice::TroppeChiamate => StatusCode::TOO_MANY_REQUESTS,
        // --- 2.I ---
        Codice::OauthStatoNonValido => StatusCode::BAD_REQUEST,
        Codice::OauthFallito => StatusCode::BAD_GATEWAY,
        // --- E (aggiornamenti) ---
        Codice::AggiornamentiDisattivati => StatusCode::FORBIDDEN,
        Codice::SorgenteSconosciuta | Codice::InCorso => StatusCode::CONFLICT,
        // --- fine S2 ---
    }
}

impl IntoResponse for ErroreHttp {
    fn into_response(self) -> Response {
        let stato = stato_http(self.0.codice);
        if stato.is_server_error() {
            tracing::error!(codice = self.0.codice.come_testo(), testo = %self.0.testo, "errore interno");
        }
        let corpo = serde_json::json!({
            "errore": { "codice": self.0.codice, "testo": self.0.testo }
        });
        (stato, Json(corpo)).into_response()
    }
}

/// Corpo JSON con errori nel formato di XBot.
pub struct Corpo<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for Corpo<T> {
    type Rejection = ErroreHttp;

    async fn from_request(req: Request, stato: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(req, stato)
            .await
            .map(|Json(v)| Corpo(v))
            .map_err(|e| {
                if e.status() == StatusCode::PAYLOAD_TOO_LARGE {
                    ErroreHttp(Errore::nuovo(
                        Codice::CorpoTroppoGrande,
                        format!("il corpo supera {} MB", LIMITE_CORPO / (1024 * 1024)),
                    ))
                } else {
                    ErroreHttp(Errore::non_valida(e.body_text()))
                }
            })
    }
}

/// Parametri di query con errori nel formato di XBot.
pub struct Domanda<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequestParts<S> for Domanda<T> {
    type Rejection = ErroreHttp;

    async fn from_request_parts(parts: &mut Parts, stato: &S) -> Result<Self, Self::Rejection> {
        Query::<T>::from_request_parts(parts, stato)
            .await
            .map(|Query(v)| Domanda(v))
            .map_err(|e| ErroreHttp(Errore::non_valida(e.body_text())))
    }
}
