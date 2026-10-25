//! Le rotte dell'API v1.

pub mod assistenti;
pub mod flusso;
pub mod invii;
pub mod sessioni;
pub mod varie;
// --- S1 ---
pub mod allegati;
pub mod assistenti_extra;
pub mod cassaforte;
pub mod macchine;
pub mod permessi;
pub mod push;
pub mod richieste;
pub mod routine;
pub mod utilizzo;
pub mod versione;
// --- fine S1 ---
// --- S2 ---
pub mod canali;
pub mod collegamenti;
pub mod voce;
// --- fine S2 ---
// --- S-B ---
pub mod annidate;
pub mod chiamate;
pub mod squadra;
// --- fine S-B ---
// --- 2.H ---
pub mod computer;
pub mod pc_windows;
// --- fine 2.H ---
// --- 2.I ---
pub mod catalogo;
pub mod oauth;
pub mod provider;
// --- fine 2.I ---

use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use xbot_core::Risposta;
use xbot_db::Connessione;

use crate::errore_http::RisHttp;
use crate::stato::Stato;

/// Esegue `f` sul database e risponde in JSON.
pub(crate) async fn json_db<T, F>(s: &Stato, f: F) -> RisHttp<T>
where
    T: Send + 'static,
    F: FnOnce(&Connessione) -> Risposta<T> + Send + 'static,
{
    Ok(Json(s.db.esegui(f).await?))
}

/// Le rotte protette della Fase 2.D (sotto `/api/v1`, con accesso). Ogni costruttore aggiunge
/// le sue nel suo blocco.
pub fn aggiunte() -> Router<Stato> {
    let r = Router::new();
    // --- S1 ---
    let r = r.merge(rotte_s1());
    // --- fine S1 ---
    // --- S2 ---
    let r = r.merge(rotte_s2());
    // --- fine S2 ---
    // --- S-B ---
    let r = r.merge(rotte_sb());
    // --- fine S-B ---
    // --- accesso fidato ---
    let r = r.merge(rotte_fidati());
    // --- fine accesso fidato ---
    // --- 2.H ---
    let r = r.merge(rotte_computer());
    // --- fine 2.H ---
    // --- 2.I ---
    let r = r.merge(rotte_2i());
    // --- fine 2.I ---
    // --- Computer Windows ---
    let r = r.merge(rotte_pc_windows());
    // --- fine Computer Windows ---
    r
}

// --- 2.I ---
/// OAuth dei collegamenti, accesso ai client dei provider, provider acceso/spento.
fn rotte_2i() -> Router<Stato> {
    use self::{oauth as oa, provider as pr};
    Router::new()
        .route("/collegamenti/{id}/oauth/avvia", post(oa::avvia))
        .route("/collegamenti/{id}/oauth", delete(oa::scollega))
        .route(
            "/collegamenti/{id}/oauth/client",
            axum::routing::put(oa::client),
        )
        .route("/provider/{id}", axum::routing::patch(pr::aggiorna))
        .route("/provider/{id}/accedi", post(pr::accedi))
        .route("/provider/{id}/esci", post(pr::esci))
        .route("/provider/{id}/accesso", get(pr::accesso).post(pr::codice))
}

/// Il ritorno OAuth: controlla da sé il cookie (senza risponde con una pagina HTML).
pub fn rotte_2i_pubbliche() -> Router<Stato> {
    Router::new().route("/oauth/ritorno", get(oauth::ritorno))
}
// --- fine 2.I ---

// --- Computer Windows ---
/// Terminale (ttyd) e schermo (noVNC) del PC dell'amministrazione, solo se `XBOT_PC_WINDOWS` è impostata.
fn rotte_pc_windows() -> Router<Stato> {
    use self::pc_windows as pc;
    use axum::routing::any;
    Router::new()
        .route("/pc-windows", get(pc::leggi))
        .route("/pc-windows/terminale", any(pc::terminale))
        .route("/pc-windows/terminale/", any(pc::terminale))
        .route("/pc-windows/terminale/{*resto}", any(pc::terminale))
        .route("/pc-windows/schermo/{*resto}", any(pc::schermo))
}
// --- fine Computer Windows ---

// --- 2.H ---
/// La scheda Computer: stato, accendi/spegni/ricrea, schermo (WebSocket) e terminale (ttyd).
fn rotte_computer() -> Router<Stato> {
    use self::computer as co;
    use axum::routing::any;
    Router::new()
        .route("/assistenti/{id}/computer", get(co::leggi))
        .route("/assistenti/{id}/computer/accendi", post(co::accendi))
        .route("/assistenti/{id}/computer/spegni", post(co::spegni))
        .route("/assistenti/{id}/computer/ricrea", post(co::ricrea))
        .route("/assistenti/{id}/computer/schermo", get(co::schermo))
        .route("/assistenti/{id}/computer/terminale", any(co::terminale))
        .route("/assistenti/{id}/computer/terminale/", any(co::terminale))
        .route(
            "/assistenti/{id}/computer/terminale/{*resto}",
            any(co::terminale),
        )
}
// --- fine 2.H ---

// --- S1 ---
/// Cassaforte, assistenti (avatar, profilo, abilità, routine, memoria, file), richieste,
/// permessi, macchine, utilizzo, versione, eventi, push, allegati e spostamento.
fn rotte_s1() -> Router<Stato> {
    use self::{allegati as al, assistenti_extra as ax, cassaforte as cf, macchine as mc};
    const MB: usize = 1024 * 1024;
    Router::new()
        .route("/cassaforte", get(cf::elenca).post(cf::crea))
        .route("/cassaforte/cerca", post(cf::cerca))
        .route(
            "/cassaforte/importa",
            post(cf::importa).layer(DefaultBodyLimit::max(10 * MB)),
        )
        .route(
            "/cassaforte/{id}",
            get(cf::leggi).patch(cf::aggiorna).delete(cf::elimina),
        )
        .route("/cassaforte/{id}/rivela", post(cf::rivela))
        .route("/cassaforte/{id}/usi", get(cf::usi))
        .route(
            "/assistenti/{id}/avatar",
            post(ax::avatar)
                .delete(ax::togli_avatar)
                .layer(DefaultBodyLimit::max(3 * MB)),
        )
        .route("/blob/{hash}", get(ax::blob))
        .route(
            "/assistenti/{id}/profilo",
            get(ax::profilo).put(ax::scrivi_profilo),
        )
        .route(
            "/assistenti/{id}/abilita",
            get(ax::abilita).post(ax::crea_abilita),
        )
        .route(
            "/assistenti/{id}/abilita/{nome}",
            get(ax::leggi_abilita)
                .patch(ax::aggiorna_abilita)
                .delete(ax::elimina_abilita),
        )
        .route("/assistenti/{id}/memoria", get(ax::memoria))
        .route("/assistenti/{id}/file", get(ax::file))
        .route(
            "/assistenti/{id}/routine",
            get(routine::elenca).post(routine::crea),
        )
        .route(
            "/routine/{id}",
            get(routine::leggi)
                .patch(routine::aggiorna)
                .delete(routine::elimina),
        )
        .route("/routine/{id}/esegui", post(routine::esegui))
        .route("/routine/{id}/esecuzioni", get(routine::esecuzioni))
        .route("/richieste", get(richieste::elenca))
        .route("/richieste/{id}/risolvi", post(richieste::risolvi))
        .route("/permessi", get(permessi::elenca))
        .route("/permessi/{id}", delete(permessi::ritira))
        .route(
            "/regole-revisione",
            get(permessi::regole).put(permessi::scrivi_regole),
        )
        .route("/macchine", get(mc::elenca))
        .route("/macchine/biglietto", post(mc::biglietto))
        .route("/macchine/{id}/lavori", get(mc::lavori))
        .route(
            "/macchine/{id}/permesso",
            get(mc::permesso).put(mc::scrivi_permesso),
        )
        .route("/utilizzo", get(utilizzo::utilizzo))
        .route("/versione", get(versione::versione))
        .route("/eventi", get(versione::eventi))
        .route("/push/chiave", get(push::chiave))
        .route(
            "/push/abbonamento",
            post(push::abbonati).delete(push::disdici),
        )
        .route(
            "/sessioni/{id}/allegati",
            post(al::carica).layer(DefaultBodyLimit::max(26 * MB)),
        )
        .route("/sessioni/{id}/sposta", post(al::sposta))
}
// --- fine S1 ---

// --- S2 ---
/// Collegamenti (server MCP), canali (token e registro) e voce.
fn rotte_s2() -> Router<Stato> {
    use self::{canali as ca, collegamenti as co};
    Router::new()
        .route("/collegamenti", get(co::elenca).post(co::crea))
        .route("/collegamenti/catalogo", get(catalogo::catalogo))
        .route(
            "/collegamenti/{id}",
            get(co::leggi).patch(co::aggiorna).delete(co::elimina),
        )
        .route("/collegamenti/{id}/prova", post(co::prova))
        .route(
            "/collegamenti/{id}/esclusi",
            axum::routing::put(co::esclusi).post(co::esclusi),
        )
        .route(
            "/collegamenti/{id}/permessi",
            axum::routing::put(co::permessi),
        )
        .route("/canali", get(ca::elenca).post(ca::crea))
        .route("/canali/{id}", delete(ca::revoca).patch(ca::aggiorna))
        .route("/canali/{id}/rigenera", post(ca::rigenera))
        .route("/canali/{id}/chiamate", get(ca::chiamate))
        .route(
            "/trascrivi",
            post(voce::trascrivi).layer(DefaultBodyLimit::max(voce::LIMITE_AUDIO + 1024 * 1024)),
        )
        .route("/sintesi", post(voce::sintesi))
        .route("/voci", get(voce::voci))
}
// --- fine S2 ---

// --- S-B ---
/// Fase 2.E: squadra, rimozione dei computer, lavoratore da scaricare. `collega` e `canale`
/// stanno fuori dall'accesso (vedi `rotte_sb_pubbliche`).
fn rotte_sb() -> Router<Stato> {
    Router::new()
        .route("/squadra", get(squadra::elenca))
        .route(
            "/sessioni/{id}/figlie",
            get(annidate::elenco).post(annidate::apri),
        )
        .route("/lavagna", get(annidate::leggi))
        .route("/lavagna/{id}/chiudi", post(annidate::chiudi))
        .route("/sessioni/{id}/chiamate", post(chiamate::crea))
        .route("/righe/{id}/voto", post(chiamate::voto_riga))
        .route(
            "/sessioni/{id}/righe/{riga_id}/voto",
            post(chiamate::voto_riga_sessione),
        )
        .route("/turni/{id}/sulla-vps", post(macchine::sulla_vps))
        .route("/macchine/{id}", delete(macchine::elimina))
        .route(
            "/scarica/xbot-worker-macos",
            get(macchine::scarica_lavoratore),
        )
}

/// Le rotte del lavoratore, senza accesso: il biglietto (`collega`) o la credenziale (`canale`).
pub fn rotte_sb_pubbliche() -> Router<Stato> {
    Router::new()
        .route("/macchine/collega", post(macchine::collega))
        .route("/macchine/{id}/canale", get(crate::canale_macchina::canale))
}
// --- fine S-B ---

// --- accesso fidato ---
/// Elenco e revoca dei dispositivi fidati (cookie `xbot_fidato`).
fn rotte_fidati() -> Router<Stato> {
    use crate::accesso as acc;
    Router::new()
        .route(
            "/dispositivi-fidati",
            get(acc::fidati_elenco).delete(acc::fidati_revoca_tutti),
        )
        .route("/dispositivi-fidati/{id}", delete(acc::fidato_revoca))
}
// --- fine accesso fidato ---
