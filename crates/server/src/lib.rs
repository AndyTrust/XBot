//! Il server di XBot: accesso, API v1, flusso SSE, motore dei turni, file statici.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod accesso;
pub mod aggiornamenti;
pub mod bus;
pub mod cliente;
pub mod config;
pub mod errore_http;
pub mod limite;
pub mod motore;
pub mod registro;
pub mod rotte;
pub mod statici;
pub mod stato;
mod traduttore;
// --- S1 ---
pub mod orologio;
pub mod profili;
pub mod push;
pub mod richieste;
pub mod servizi;
pub mod sistema;
// --- fine S1 ---
// --- S2 ---
pub mod mcp_canale;
pub mod mcp_interno;
pub mod menzioni;
pub mod posta;
pub mod voce;
// --- fine S2 ---
// --- S-B ---
pub mod canale_macchina;
// --- fine S-B ---
// --- 2.H ---
pub mod computer;
// --- fine 2.H ---
// --- 2.I ---
pub mod accessi;
pub mod oauth;
// --- 2.K --- la prova dei collegamenti sulla macchina dove girano.
pub mod prova_remota;
// --- 2.L --- quote scadute e rilettura.
pub mod quote_scadute;
// --- fine 2.I ---

use std::path::Path;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::middleware::from_fn_with_state;
use axum::routing::{any, get, post};
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;

pub use stato::Stato;

/// Il router completo: API v1 sotto `/api/v1`, interfaccia alla radice e su `/app/*`.
pub fn app(stato: Stato, web: &Path) -> Router {
    use rotte::{assistenti, flusso, invii, sessioni, varie};

    let protetta = Router::new()
        .route("/io", get(varie::io))
        .route("/barra", get(varie::barra))
        .route(
            "/assistenti",
            get(assistenti::elenca).post(assistenti::crea),
        )
        .route(
            "/assistenti/{id}",
            get(assistenti::leggi)
                .patch(assistenti::aggiorna)
                .delete(assistenti::archivia),
        )
        .route(
            "/assistenti/{id}/sessioni",
            get(sessioni::elenca).post(sessioni::crea),
        )
        .route("/assistenti/{id}/letto", post(varie::letto))
        .route(
            "/sezioni",
            get(assistenti::sezioni)
                .post(assistenti::crea_sezione)
                .patch(assistenti::aggiorna_sezioni),
        )
        .route(
            "/sessioni/{id}",
            get(sessioni::leggi).patch(sessioni::aggiorna),
        )
        .route("/sessioni/{id}/azzera", post(sessioni::azzera))
        .route("/sessioni/{id}/righe", get(sessioni::righe))
        .route("/sessioni/{id}/flusso", get(flusso::flusso))
        .route("/sessioni/{id}/invii", post(invii::invia))
        .route("/turni/{id}/interrompi", post(invii::interrompi))
        .route("/quote", get(rotte::provider::quote))
        // --- E --- aggiornamenti per chi installa.
        .route("/aggiornamenti", get(aggiornamenti::stato))
        .route("/aggiornamenti/avvia", post(aggiornamenti::avvia))
        .route("/provider", get(rotte::provider::elenca))
        .route("/modelli", get(rotte::provider::modelli))
        .route(
            "/impostazioni",
            get(varie::impostazioni).put(varie::scrivi_impostazioni),
        )
        // Fase 2.D: le rotte di S1 e S2 stanno in `rotte::aggiunte` (blocchi per costruttore).
        .merge(rotte::aggiunte())
        .route_layer(from_fn_with_state(stato.clone(), accesso::richiede_accesso));

    // --- S2 --- il canale MCP `/mcp` (fuori da /api/v1, autenticato con il token del canale).
    let canale = mcp_canale::router(stato.clone());
    // --- fine S2 ---
    let api = Router::new()
        .route("/salute", get(varie::salute))
        .route("/accesso", post(accesso::entra).delete(accesso::esci))
        // --- S-B --- collega (biglietto) e canale (credenziale) del lavoratore.
        .merge(rotte::rotte_sb_pubbliche())
        // --- fine S-B ---
        // --- 2.I --- il ritorno OAuth controlla da sé il cookie.
        .merge(rotte::rotte_2i_pubbliche())
        .merge(protetta)
        .fallback(varie::non_trovata)
        .layer(DefaultBodyLimit::max(errore_http::LIMITE_CORPO))
        .with_state(stato);

    Router::new()
        .nest("/api/v1", api)
        // Un /api/* che non esiste risponde in JSON, mai con index.html.
        .route("/api", any(varie::non_trovata))
        .route("/api/{*resto}", any(varie::non_trovata))
        // --- S2 ---
        .merge(canale)
        // --- fine S2 ---
        .merge(statici::router(web))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
}
