# XBot: architettura del codice

*Fissata il 2026-10-24 15:20 dalla chat master, dopo l'ok dell'autore su schema e layout. Vale per la Fase 2. Chi costruisce la segue; se qualcosa non torna lo scrive nel resoconto, non la cambia da solo.*

Documenti di riferimento (sola lettura):
- `schema.sql` (qui): i dati, versione 1. Si usa così com'è; una modifica allo schema è una migrazione nuova, non un ritocco.
- La specifica di comportamento (sezioni 12 e 14: i flussi), il layout dell'interfaccia e il piano di costruzione stanno nei documenti di progetto dell'autore, fuori dal repository.

## 1. Regole del codice

- Rust edition 2024, toolchain stable 1.99. Workspace con `resolver = "3"`.
- `unsafe_code = "forbid"` nel workspace. In ogni `lib.rs`: `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`. Nei test `unwrap` è ammesso (`clippy.toml`: `allow-unwrap-in-tests = true`).
- `cargo clippy --all-targets -- -D warnings` e `cargo fmt --check` puliti. `cargo test` verde. Niente `dbg!`.
- File sotto 500 righe (700 al massimo). Nomi di tipi, funzioni, moduli, rotte e tabelle **in italiano**, come lo schema. I crate esterni si chiamano come si chiamano.
- Nessun segreto nel codice, nei log o nei file di impostazione. I valori sensibili passano per `SegretoTesto` (niente `Debug` in chiaro, niente `Serialize`).
- Tempi in millisecondi dall'epoca, `i64`. Id testuali ULID (`ulid` crate) dove lo schema dice TEXT PRIMARY KEY.
- Log con `tracing`; livello da `XBOT_LOG`. Mai il testo dei messaggi nei log, solo id e lunghezze.
- Gli artefatti di compilazione stanno fuori dalla cartella sincronizzata: un `.cargo/config.toml` locale, non tracciato, fissa `target-dir` su una cartella della macchina.

## 2. I crate

```
XBot/
├── Cargo.toml                workspace
├── schema.sql                dati v1 (incorporato in xbot-db con include_str!)
├── crates/
│   ├── core/      xbot-core      tipi condivisi, errori, id, SegretoTesto, orologio
│   ├── db/        xbot-db        apertura SQLite, migrazioni, repository (una funzione per operazione)
│   ├── provider/  xbot-provider  trait Provider + adattatori: finto (per i test), claude_code (Fase 2.B)
│   └── server/    xbot-server    axum: accesso, API v1, flusso SSE, file statici dell'interfaccia
└── web/                             interfaccia (Fase 2.C): HTML, CSS e JS semplici serviti dal server
```

### xbot-core
- `Id` (ULID), `Ms` (i64), `SegretoTesto`, `Errore` (enum con `Codice` testuale stabile, es. `sessione_non_trovata`, `modello_non_disponibile`, `invio_duplicato`, `provider_fermo`), `Risposta<T>`.
- Enumerazioni dello schema come tipi Rust con `serde(rename_all = "snake_case")`: `GenereRiga`, `Ruolo`, `StatoInvio`, `Consegna`, `StatoTurno`, `StatoLavoro`, `PermessoEsecuzione`, `TipoRichiesta`, `StatoRichiesta`, `GenereCassaforte`.

### xbot-db
- `apri(percorso) -> Result<Db>`: `PRAGMA foreign_keys=ON`, `journal_mode=WAL`, `busy_timeout=5000`; applica `schema.sql` se `schema_meta` manca; versione in `schema_meta.schema_version`.
- Un `Db` con pool semplice (una connessione in `Mutex` va bene per la prima versione; tutte le chiamate dal server passano per `tokio::task::spawn_blocking`).
- Moduli repository, uno per gruppo dello schema: `accesso`, `provider`, `assistenti`, `sessioni`, `righe`, `invii`, `turni`, `richieste`, `macchine`, `permessi`, `cassaforte`, `collegamenti`, `routine`, `impostazioni`, `notifiche`, `eventi`.
- Regole dei dati:
  - `righe::aggiungi(sessione, genere, ruolo, corpo)`: prende `ultimo_seq+1` e `ultimo_aggiornata_seq+1` nella stessa transazione, aggiorna `sessione`.
  - `righe::riscrivi(riga_id, corpo)`: stesso `seq`, `aggiornata_seq = ultimo_aggiornata_seq+1`.
  - `righe::dopo(sessione, generazione, dopo_aggiornata_seq, limite)`: se `generazione` è vecchia → `Errore::cursore_vecchio`.
  - `invii::crea`: se l'id esiste già → stato `duplicato` senza creare nulla (idempotenza).
  - `turni::chiudi(id, stato, token, costo)` scrive anche `durata_ms`.
- Test con database in memoria (`:memory:`) per ogni modulo: almeno uno per la regola sopra, uno per la vista `v_barra_laterale`, uno per la cassaforte (cifra, decifra, manomissione rifiutata).

### xbot-provider
```rust
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;                                   // 'claude_code' | 'grok' | 'cursor_agent' | 'finto'
    async fn modelli(&self) -> Result<Vec<Modello>, Errore>;
    async fn quota(&self) -> Result<Option<Quota>, Errore>; // None = non leggibile dal client (si stima dai turni)
    async fn avvia(&self, richiesta: RichiestaTurno, eventi: mpsc::Sender<EventoTurno>) -> Result<ManigliaTurno, Errore>;
}
pub struct RichiestaTurno { sessione_id, sessione_esterna_id: Option<String>, modello: Option<String>, livello: Option<String>, cartella_lavoro: Option<PathBuf>, testo: String, allegati: Vec<PathBuf>, sistema: Option<String> /* profilo + memoria */ }
pub enum EventoTurno { Avviato { sessione_esterna_id: Option<String> }, TestoDelta(String), TestoCompleto(String), Ragionamento(String), ChiamataStrumento { nome, input_json }, EsitoStrumento { nome, esito_json, ok }, Avviso(String), Finito { token_ingresso, token_uscita, token_cache, costo_stimato_eur: Option<f64>, sessione_esterna_id: Option<String> }, Fallito { codice, testo } }
pub struct ManigliaTurno { pub pid: Option<u32>, pub interrompi: Box<dyn FnOnce() + Send> }
```
- Adattatore **finto** (`finto.rs`): risponde con il testo rovesciato in tre delta a 50 ms, un `ChiamataStrumento` di prova, poi `Finito` con token contati sulle parole. Se il testo contiene `[fallisci]` manda `Fallito`. Serve ai test del server e dell'interfaccia senza spendere abbonamento.
- Adattatore **claude_code** (Fase 2.B, non in questa consegna): lancia `claude -p --output-format stream-json --input-format stream-json --verbose --include-partial-messages [--resume <id>] [--model <m>] --permission-mode <modo> --strict-mcp-config`, scrive il messaggio utente su stdin in `stream-json`, legge stdout riga per riga e traduce in `EventoTurno`; `sessione_esterna_id` = `session_id` della prima riga; `Finito` dalla riga `result` (`usage`, `total_cost_usd`); quota: `None` (si stima).

### xbot-server
- `axum` 0.8, `tokio`, `tower-http` (file statici, compressione, tracing), cookie di sessione firmato (`axum-extra` cookie), `tokio-stream` per SSE.
- Impostazioni da variabili d'ambiente con prefisso `XBOT_`: `XBOT_DB` (percorso SQLite), `XBOT_ASCOLTO` (`127.0.0.1:8787`), `XBOT_WEB` (cartella dell'interfaccia), `XBOT_SEGRETO_COOKIE` (base64, 32 byte; se manca, generato e salvato in `impostazione`), `XBOT_LOG`.
- Configurabilità per l'installazione pubblica (2026-10-25): `XBOT_NOME` (nome del bot orchestratore, predefinito «XBot»: voce predefinita, `nome_app` in `GET /api/v1/salute`, `name`/`short_name` del manifest servito); `XBOT_SESSIONI` (predefinita `<XBOT_DATI>/sessioni` sul server; sul lavoratore `sessioni` accanto alla sua cartella); `XBOT_STRUMENTI_DIR` (script esterni `quaderno.py` e `cerca_memoria.py`, predefinita `<XBOT_DATI>/strumenti`); `XBOT_MEMORIA_CARTELLA` e `XBOT_MEMORIA_NOME` (voce «memoria» del catalogo, predefiniti `~/Memoria` e «Memoria»); `XBOT_WORKER_CARTELLA` (predefinita `~/.xbot/worker`); `XBOT_ETICHETTA_WORKER` (etichetta del LaunchAgent, predefinita `it.xbot.worker`). Senza `XBOT_DATI` i provider usano `~/.xbot`. Il nome dell'utente nei testi viene dalla tabella `utente` (ripiego «utente»).
- Aggiornamenti per chi installa (2026-10-25, `CONTRATTO-GITHUB.md` sezione E, `crates/server/src/aggiornamenti.rs`): `XBOT_VERSIONE_FILE` (predefinito `<XBOT_DATI>/versione.txt`, prima parola = versione in opera, altrimenti quella di Cargo; esce in `GET /salute` come `versione_installata`); `XBOT_REPO` (predefinito `AndyTrust/XBot`); `XBOT_AGGIORNAMENTO` (`avvisa` | `auto` | `no`, predefinito `avvisa`); `XBOT_SORGENTE` (cartella del clone, scritta da `installa.sh`); `XBOT_AGGIORNA_COMANDO` (predefinito `<XBOT_SORGENTE>/installa.sh --aggiorna --si`, lanciato con `sh -c`); `XBOT_GITHUB_API` (predefinito `https://api.github.com`, solo per le prove). `GET /aggiornamenti[?forza=1]` → `{versione_installata, versione_remota, disponibile, controllato_ms, modo, note_url}` (+ `errore` se GitHub non risponde: allora `disponibile` e `versione_remota` sono `null`), cache in memoria di 6 ore; `POST /aggiornamenti/avvia` → 202 `{avviato, pid}`, 403 `aggiornamenti_disattivati`, 409 `sorgente_sconosciuta` o `in_corso`; uscita del comando in `<XBOT_DATI>/aggiornamenti.log`.
- Comando `xbot-server impianta --nome <utente> --password … --totp` che crea l'utente (password con Argon2id in `utente.password_cifrato`? → **no**: aggiungere la colonna richiede una migrazione; per la versione 1 la password sta in `impostazione` con chiave `accesso.password_hash` e il segreto TOTP in `utente.totp_cifrato`). Stampa l'URL `otpauth://` e il QR in ASCII.
- Rotte API v1, tutte sotto `/api/v1`, JSON, errori `{ "errore": { "codice": "...", "testo": "..." } }` con lo stato HTTP giusto:

| Metodo e rotta | Cosa fa | Tabelle |
|---|---|---|
| `POST /accesso` `{password, codice_totp, dispositivo}` | apre la sessione web, cookie `xbot_accesso` (HttpOnly, SameSite=Lax, Secure fuori da localhost) | `accesso_web` |
| `POST /accesso` con `fidato: true` | dopo password + TOTP rilascia anche il cookie firmato `xbot_fidato` (30 giorni, 32 byte casuali, hash sha256 in `dispositivo_fidato`, migrazione 008); con quel cookie valido basta la password. Senza cookie e senza codice: 401 `totp_richiesto` | `dispositivo_fidato` |
| `GET /dispositivi-fidati` · `DELETE /dispositivi-fidati/{id}` (204) · `DELETE /dispositivi-fidati` (`{revocati}`) | elenco con `corrente`, revoca di uno o di tutti | `dispositivo_fidato` |
| `DELETE /accesso` | esce (non tocca il dispositivo fidato) | |
| `GET /io` | chi sono, impostazioni, versione | `utente`, `impostazione` |
| `GET /barra` | la barra laterale (vista `v_barra_laterale`) con sezioni, assistenti, non letti, in attesa | |
| `GET,POST /assistenti`, `GET,PATCH,DELETE /assistenti/{id}` | assistenti; DELETE = archivia | `assistente`, `sezione_assistente` |
| `GET,POST,PATCH /sezioni` | sezioni della barra, ordine | `sezione_barra` |
| `GET /assistenti/{id}/sessioni`, `POST /assistenti/{id}/sessioni` | sessioni; POST crea con provider/modello/macchina | `sessione` |
| `GET /sessioni/{id}`, `PATCH /sessioni/{id}` (titolo, provider, modello, livello, macchina, archivia), `POST /sessioni/{id}/azzera` (nuova generazione) | | |
| `GET /sessioni/{id}/righe?dopo_aggiornata_seq=&generazione=&limite=` | lettura a cursore; `cursore_vecchio` se la generazione è cambiata | `riga` |
| `GET /sessioni/{id}/flusso` (SSE) | frame `connected{stream_id, server_ms, vita_ms}`, `rows{generazione, righe[]}`, `cleared{nuova_generazione}`, `cursor_too_old`, `heartbeat` ogni 15 s, `turno{stato}`; parte da `dopo_aggiornata_seq` | |
| `POST /sessioni/{id}/invii` `{id, testo, allegati, risposta_a, provider, macchina}` | crea `invio` (idempotente per id), la riga `messaggio` dell'utente, un `turno` e lancia il provider; risponde `{stato, consegna, riga_eco_id, turno_id}` | `invio`, `riga`, `turno` |
| `POST /turni/{id}/interrompi` | ferma il processo | `turno` |
| `GET /quote` | `v_quota_corrente` più stima dai turni degli ultimi 7 giorni | |
| `GET /provider`, `GET /modelli` | elenco | |
| `GET,PUT /impostazioni` | `{"valori": {chiave: valore}}` (dalla 2.B-bis) | `impostazione` |
| `POST /assistenti/{id}/letto` `{fino_seq}` | segna letto | `stato_lettura` |
| `GET /salute` | `{ok, versione, db}` senza accesso | |

- Il **motore dei turni** (`motore.rs`): riceve l'invio, sceglie il provider (per ora quello della sessione o dell'assistente; «auto» = primo attivo), costruisce `RichiestaTurno` con il profilo dell'assistente (file `profilo_percorso`) e la memoria (contenuto di `Memoria.md` e della nota del progetto, se `memoria_cartella` c'è), avvia, e traduce gli `EventoTurno` in righe: `TestoDelta` riscrive la stessa riga `messaggio` dell'assistente (`righe::riscrivi`), `ChiamataStrumento` aggiunge una riga `chiamata_strumento`, `Finito` chiude il turno e aggiorna `stato_lettura.in_attesa_utente=0`, `Fallito` aggiunge una riga `avviso` e chiude il turno `fallito`. Un solo turno per sessione alla volta: un secondo invio con turno in corso va in `consegna = in_coda_prossimo_turno` e parte alla chiusura.
- Un **bus** (`tokio::sync::broadcast`) per sessione: ogni riga nuova o riscritta pubblica `(sessione_id, aggiornata_seq)`; il flusso SSE legge dal bus e rilegge dal database (mai dati dal bus: solo il segnale).
- File statici: `GET /` e `/app/*` servono `web/` con fallback a `index.html`; `GET /manifest.webmanifest`, `GET /sw.js` (Fase 2.C).

## 3. Prove richieste (Fase 2.A)

1. `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` puliti.
2. Test di integrazione del server con `tower::ServiceExt::oneshot` o con un server su porta libera: accesso con password e TOTP giusti e sbagliati; creazione assistente e sessione; invio con provider `finto`; lettura delle righe a cursore che vede la riga riscritta con `aggiornata_seq` crescente; invio duplicato → `duplicato`; `cursore_vecchio` dopo `azzera`; SSE che consegna `connected` e almeno un `rows`.
3. Avvio a mano: `xbot-server impianta …`, poi `xbot-server` su `127.0.0.1:8787`; `curl` di `/api/v1/salute` e del giro completo (accesso → assistente → sessione → invio → righe). I comandi e le uscite vanno nel resoconto.

## 4. Fase 2.B e 2.C (dopo, non ora)
- 2.B: adattatore `claude_code` con un `claude` finto (script che stampa righe `stream-json` registrate) nei test e il `claude` vero a mano; quota stimata dai turni; `grok` e `cursor_agent` dopo.
- 2.C: interfaccia in `web/` dal Layout: barra, chat, barra di scrittura con provider e anello, pannello; HTML, CSS e JS senza framework per la prima versione; PWA (manifest, service worker, push) subito dopo.
- 2.D: macchine e lavori (protocollo Device), cassaforte, collegamenti, canali MCP, routine, voce.

## 5. Fase 2.B eseguita

*2026-10-24 15:45, costruttore-xbot. 62 test verdi, clippy `-D warnings` e fmt puliti.*

- **Adattatore `claude_code`** (`crates/provider/src/claude_code.rs` per il processo, `claude_code/flusso.rs` per la traduzione delle righe, senza processo). Argomenti: `-p --output-format stream-json --input-format stream-json --verbose --include-partial-messages --strict-mcp-config --settings '{"disableAllHooks":true}' --permission-mode <modo>` più `--resume`, `--model`, `--effort` (solo `low|medium|high|max`), `--append-system-prompt` (esiste in Claude Code 2.1.310). Il messaggio va su stdin in una riga `{"type":"user",...}`, poi stdin si chiude; gli allegati passano come `[allegato: <percorso>]` in coda al testo. Toglie dall'ambiente del figlio le variabili di una sessione Claude Code madre (`CLAUDECODE`, `CLAUDE_CODE_SESSION_ID`, socket di messaggistica…).
- **Traduzione**: `system/init` → `Avviato`; `text_delta` → `TestoDelta`; `thinking_delta` → `Ragionamento`; `tool_use` + `input_json_delta` → `ChiamataStrumento` alla chiusura del blocco; `tool_result` → `EsitoStrumento` (nome dal `tool_use_id`); `assistant` → `TestoCompleto` con il testo della bolla; `rate_limit_event.unifiedWindows` → `Quota` (`five_hour`, poi `seven_day`, `resetsAt` × 1000); `result` → `Finito` (`token_ingresso = input + cache_read + cache_creation`, `token_cache = cache_read`, `costo_usd`) o `Fallito{codice: subtype}`. Le righe con `parent_tool_use_id` (sottoagenti) si ignorano. Uscita senza `result` → `Fallito{processo_uscito}` con la coda di stderr. 10 minuti senza righe → `Fallito{silenzio}`. `interrompi` = SIGTERM, SIGKILL dopo 5 s (crate `nix`).
- **Tipi**: `RichiestaTurno.permessi` (predefinito `acceptEdits`), `EventoTurno::Quota{finestra, utilizzo, azzera_ms}`, `Finito.costo_usd` al posto di `costo_stimato_eur`.
- **Motore**: a `Quota` il traduttore ricorda ogni finestra e alla `seven_day` scrive una `quota_rilevazione` (fonte `client`, percentuale = utilizzo × 100, `dettaglio_json` con tutte le finestre). A `Finito` il costo in dollari va nella colonna `turno.costo_stimato_eur` (nome da decidere). A turno riuscito `in_attesa_utente = 1`, `al_lavoro = 0`; «letto» e un nuovo turno lo riportano a 0.
- **Registro**: `servi` carica dalla tabella `provider` quelli `attivo = 1` (`finto`, `claude_code`); `impianta` registra `claude_code` (claude dal `PATH` senza shell, o `--claude`) con i tre modelli; `--finto` registra il finto. `XBOT_SESSIONI` sposta le cartelle vuote delle sessioni.
- **Migrazioni**: la 001 è `schema.sql` (incluso com'è); `crates/db/migrazioni/002.sql` rifà `v_barra_laterale` con i non letti per `creata_ms > ultima_vista_ms`, `soppressa = 0`; `schema_version` = 2. `migra()` applica ogni migrazione in un savepoint.
- **Accesso**: 5 tentativi falliti in 5 minuti per indirizzo del client e 20 in tutto (in memoria, `limite.rs`) → 429 `troppi_tentativi`; un accesso riuscito azzera il conto di quell'indirizzo. L'indirizzo viene da `cliente.rs`: dietro un proxy in `XBOT_PROXY_FIDATO` è l'ultimo non fidato di `X-Forwarded-For` (o `X-Real-IP`), altrimenti quello della connessione. `XBOT_COOKIE_SICURO=1` forza il cookie `Secure` dietro Caddy.
- **Prove**: `claude` finto in Python (`crates/provider/prove/claude-finto`) sul campione registrato; test del provider (turno completo, resume, strumento, fallimento, crollo, interruzione, silenzio) e del server (invio completo con quota, token, «tocca a te», resume; errore e strumento; accesso bloccato). Aiuti dei test in `crates/server/tests/comune/`.
- **Prova vera** (2026-10-24 15:40, haiku, porta 8793): «Rispondi solo con la parola: ciao» → `ciao`; quota `seven_day` 25 % fonte `client`, `five_hour` 14 %; secondo invio con `--resume` → `ciao`, stesso `session_id`. Costo dei due turni: 0,0076 + 0,0109 USD.

## 6. Fase 2.B-bis eseguita

*2026-10-24 15:55, costruttore-xbot. Difetti della verifica 2.A (rapporto dell'autore, fuori dal repository). 71 test verdi, clippy `-D warnings` e fmt puliti. Schema alla versione 3.*

- **TOTP una volta sola**: il passo accettato si ricorda in `impostazione` `accesso.totp_ultimo_passo`; un codice dello stesso passo o di uno precedente → 401 `accesso_negato`.
- **Riferimenti** (`crates/db/src/controlli.rs`): prima di scrivere si controllano provider, modello, sezione, macchina, progetto, assistente, sessione. Codici: `provider_non_trovato` 404, `sezione_non_trovata` 404, `assistente_non_trovato` 404, `sessione_non_trovata` 404, `modello_non_disponibile` 409, `riferimento_non_trovato` 400 (macchina, progetto e ogni vincolo di chiave esterna sfuggito ai controlli). Il testo di SQLite non esce più.
- **Assistente archiviato**: invii e sessioni nuove → 409 `assistente_archiviato`; le sessioni esistenti e le loro righe restano leggibili.
- **Limiti**: nome assistente 80, titolo 120, descrizione 2000, titolo sessione 200, testo dell'invio 100 000, chiave d'impostazione 80 caratteri → 400 `testo_troppo_lungo`. Corpo oltre 2 MB (`DefaultBodyLimit`) → 413 `corpo_troppo_grande`.
- **Stato dell'invio a fine turno** (`motore/chiusura.rs`): `invio.stato = 'consegnato'` quando il turno finisce, comunque finisca (`riuscito`, `fallito`, `interrotto`); `fallito` solo se il provider non è mai partito (provider non caricato, `avvia` fallito, turno interrotto prima di partire, invio che non trova provider). Un invio in coda resta `accettato` finché il suo turno non chiude.
- **Codice dell'errore**: `turno.errore_codice` = il codice del `Fallito` del provider (`finto_fallito`, `error_during_execution`, `silenzio`, `processo_uscito`, `provider_chiuso`); lo stesso codice sta nella riga `avviso`.
- **Impostazioni**: `GET /impostazioni` → `{"valori": {chiave: valore_json}}`; `PUT /impostazioni` vuole `{"valori": {...}}` (altri campi: 400), scrive solo le chiavi date e risponde con tutte le pubbliche. Chiavi `accesso.*` e `server.*`: sempre 403 `chiave_riservata`, mai lette.
- **Chiusura**: a SIGTERM o Ctrl-C ogni flusso SSE riceve il frame `chiusura{server_ms}` e finisce; il server esce in 0,03 s (prova a mano). Il ripiego dei 3 secondi resta.
- **Migrazione 003**: `turno.costo_stimato_eur` → `costo_usd` (anche in `Turno`, `Conto`, `GET /quote` `costo_7g`). `schema.sql` resta la versione 1.
- **Sezioni**: `PATCH /sezioni` con `[{id, nome?, ordine?, chiusa?}]` riordina in una transazione; una sezione inesistente annulla tutto con 404 `sezione_non_trovata`.

## 7. Fase 2.C-bis eseguita (server)

*2026-10-24 16:10, costruttore-xbot. 75 test verdi.*

- **File statici** (`crates/server/src/statici.rs`): `web/` si serve alla radice (`/`, `/stile.css`, `/app.js`, `/sw.js`, `/manifest.webmanifest`, `/icone/*`) e sotto `/app/*`; un percorso che non è un file dà `index.html`. `/api/v1/*` ha la precedenza; un `/api/*` che non esiste risponde 404 JSON. `/sw.js` e `/app/sw.js` portano `Service-Worker-Allowed: /`.
- **Barra**: ogni assistente ha `anteprima` (ultima riga `messaggio` non soppressa delle sue sessioni, ruolo qualsiasi, senza Markdown, al massimo 120 caratteri con «…») e `anteprima_ms` (`creata_ms` della riga). Query a parte in `crates/db/src/barra.rs`; la vista non cambia.
- **`GET /io`**: `collegamenti_accesi` = collegamenti con `abilitato = 1`; i provider senza `programma` (il percorso non esce).
- **Provider finto**: a ogni turno dichiara `Quota{seven_day, 0.27, ora + 4 giorni}`; il motore la registra come ogni quota del client.

## 8. Fase 2.D S1

*2026-10-24 18:55, costruttore S1. Schema alla versione 4. Prove S1: `tests/s1_cassaforte.rs`, `tests/s1_assistenti.rs`, `tests/s1_motore.rs` (12 d'integrazione) più i test unitari dei moduli; clippy `-D warnings` pulito.*

- **Servizi** (`crates/server/src/servizi.rs`): `Interno.servizi` tiene la chiave della cassaforte (`Option<Arc<Chiave>>`, `servizi.chiave()` → 503 `cassaforte_chiusa`), le cartelle (`cartella_dati`, `profili`, `blob`), le attese delle richieste, le chiavi VAPID, la macchina del server, `avviato_ms`. `Stato::prepara_servizi(…, OpzioniServizi)`: `servi` usa `OpzioniServizi::da_ambiente`, le prove `OpzioniServizi::per_prove(Some(frase))`; `prepara_con` resta com'era (cartella temporanea, cassaforte chiusa). `servizi::avvia_compiti` fa partire battito, orologio e notificatore (solo in `servi`).
- **Cassaforte** (`xbot-db`: `cassaforte_generi.rs` regole per genere, Luhn, marca; `cassaforte_voci.rs` crea/PATCH/elimina/cerca/rivela/usi; `cassaforte_csv.rs` sei formati). La chiave si apre con `cassaforte::apri` sulla frase di `XBOT_CASSAFORTE_CHIAVE` (la prima volta `imposta_passphrase`). I campi stanno cifrati come JSON; il PATCH decifra, unisce le chiavi date (`null` toglie), rinormalizza e ricifra. L'import salta righe senza password, voci non di accesso e doppioni (titolo + utente + primo sito).
- **Blob e allegati** (`xbot-db::allegati`): `<blob>/<hh>/<sha256>`, una riga `blob` per contenuto; l'allegato ha accanto una copia con il nome (`<hash>.d/<nome>`) che il motore passa al provider. Nell'invio `allegati[]` valgono solo id di allegati della stessa sessione: un percorso scritto dal client si scarta (prima passava com'era).
- **Testo di sistema** (`sistema.rs`, chiamato dal motore nel blocco S1 di `esegui_turno`): profilo (`profilo_percorso` o `<profili>/<id>.md`), abilità attive, istruzioni fisse (altri assistenti con `@menzione` e titolo, cassaforte e i suoi due strumenti, nomi delle variabili dei segreti concessi, quaderno), riassunto di passaggio se la sessione non ha ancora la sessione esterna, memoria.
- **Richieste** (`richieste.rs`): `crea` scrive la richiesta e la riga `richiesta` in chat; `attendi(id, scadenza)` legge prima il database poi aspetta un `watch` per richiesta (alla scadenza: `scaduta`); `risolvi` chiude, riscrive la riga, sveglia chi aspetta e con `sempre` scrive `permesso_azione` (`azione` dal carico o `run-command`, `destinatario`/`strumento`, `risorsa` = `comando_approvato` o `comando`; negata + sempre = `mai`). `xbot-db::permessi::cerca` trova la concessione più specifica.
- **Orologio** (`orologio.rs`, crate `cron`): `cron` a 5 campi (ora locale) o 6-7 con i secondi, `ogni_secondi` ≥ 5, `evento` (solo a mano). Ogni secondo esegue le routine con `prossima_ms` passata: invio `routine:<id>` nella sessione corrente dell'assistente (o la più recente attiva, o una nuova), `routine_esecuzione` con l'esito dell'invio.
- **Push** (`push.rs`): `web-push-native` (RustCrypto puro) + `reqwest` con rustls al posto del crate `web-push`, che vuole OpenSSL e libcurl. Il notificatore guarda ogni 3 s turni riusciti, richieste nuove e righe `avviso`; salva in `notifica` e manda a ogni `abbonamento_push` (404/410 → abbonamento cancellato).
- **Macchine**: id stabile in `impostazione` `server.macchina_id`, riga `macchina` con `chiave_pubblica` vuota (il server non firma), capacità `["turni"]`. Biglietti in `biglietto_macchina` (hash, 15 minuti) per la Fase 2.E.
- **Versione**: `crates/server/build.rs` mette `XBOT_COMMIT` (git, o la variabile omonima al build, o «sconosciuto») e `XBOT_COMPILATO_MS`.
- **Divisione dei file condivisi**: rotte in `rotte::aggiunte()` (blocchi `// --- S1 ---`/`// --- S2 ---`), unita alle rotte protette in `lib.rs`.

## 9. Fase 2.D S2

*2026-10-24 19:05, costruttore S2. Nessuna migrazione. Prove S2: `tests/s2_mcp.rs`, `s2_menzioni.rs`, `s2_collegamenti.rs`, `s2_canali.rs`, `s2_voce.rs` (9 d'integrazione) più i test unitari dei moduli; clippy `-D warnings` e fmt puliti.*

- **Claude Code con MCP** (`xbot-provider::claude_code::mcp`): `RichiestaTurno.mcp: Option<ConfigMcp>` e `RichiestaTurno.ambiente: Vec<(String, SegretoTesto)>`. Con `mcp` il provider scrive il `--mcp-config` (0600) e lancia `claude … --mcp-config <file> --permission-mode default --permission-prompt-tool mcp__xbot__permesso` (`--strict-mcp-config` c'era già). Formato della risposta verificato con Claude Code 2.1.310: ingresso `{tool_name, input, tool_use_id}`, uscita testo JSON `{"behavior":"allow","updatedInput":{…}}` o `{"behavior":"deny","message":"…"}`.
- **Motore** (`motore/mcp.rs`, agganci nei blocchi S2 di `motore.rs`): per i turni `claude_code` prepara `<dati>/turni/<turno>/` (fuori dalla cartella di lavoro: l'assistente non legge il file), il server `xbot` (ponte con gettone casuale in `XBOT_MCP_GETTONE`), i collegamenti abilitati dell'assistente (stdio con ambiente decifrato, http con intestazioni; nome `[a-z0-9_-]` unico) e i segreti (`cassaforte_voci::segreti_per_assistente` di S1). A fine turno cancella la cartella e, se il turno è riuscito, inoltra le menzioni del testo dell'assistente. `Motore` tiene `StatoS2` (`mcp_interno.rs`): socket, registro dei turni, chiave della cassaforte (dai servizi di S1), attesa delle richieste, configurazione della voce, limiti delle menzioni.
- **Server MCP interno** (`mcp_interno.rs`, `mcp_interno/{ponte,permesso,strumenti}.rs`, crate `rmcp` 3.7): socket Unix 0600 aperto da `Stato::prepara_servizi`; la prima riga è `{sessione, turno, gettone}`. `permesso`: strumenti `mcp__xbot__*` sempre; strumento di un collegamento → `permesso_strumento` (`sempre`/`bloccato`/`chiedi`, con `*lettura` per gli strumenti `readOnlyHint` e `*scrittura` per gli altri); poi `permessi::cerca` (S1) con azione `run-command`/`write-file`/`read-file`/`strumento_mcp`/`use-app`, destinatario = strumento (o collegamento), risorsa = comando o percorso; poi le regole di revisione (testo contenuto, minuscole, `blocca` vince); altrimenti `richieste::crea` + `richieste::attendi` (S1) per 10 minuti con «tocca a te» acceso. `comando_approvato` sostituisce il comando di Bash.
- **Menzioni** (`menzioni.rs`): le menzioni vengono da `assistenti::menzioni` di S1 (campo `menzione` di `assistenti::elenca`). Origini: `menzione:<sessione>` (dall'utente) e `collega:<sessione>:<passo>` (da un assistente). Limiti in memoria per coppia.
- **Collegamenti** (`xbot-db::collegamenti` e `collegamenti/vivi.rs`; `rotte/collegamenti.rs`): BLOB cifrati con AAD `collegamento:<id>:intestazioni|ambiente`; strumenti dell'ultima prova nell'impostazione riservata `server.collegamento.<id>.strumenti`; prova con timeout di 30 s.
- **Canali** (`xbot-db::canali`, `rotte/canali.rs`, `mcp_canale.rs`): `StreamableHttpService` di `rmcp` senza sessione (`NeverSessionManager`, risposte JSON, controllo dell'host spento: decide il token), dietro un middleware axum che mette il `CanaleAttivo` nelle estensioni della richiesta.
- **Voce** (`voce.rs`, `rotte/voce.rs`, `voce/*.py`): `ConfigVoce` letta dall'ambiente all'avvio e sostituibile nelle prove (in edition 2024 `set_var` è `unsafe`); trascrizione con timeout 120 s e file temporaneo cancellato; sintesi passata in flusso (`Body::from_stream`).
- **2.D-bis** (19:50): `controlli::testo_valido`/`nomi_validi` (codice `testo_non_valido`) nei repository di assistenti, sezioni, sessioni, routine, cassaforte, collegamenti e canali; `sistema::senza_controllo` sul testo composto e `replace('\0', "")` nell'argomento `--append-system-prompt`. `richieste::{chiudi_con, ritira}` usabili dal motore (che tiene una copia delle `Attese` in `StatoS2`); `motore/mcp.rs::s2_ritira` a ogni fine turno. Migrazione 005 `canale.limite_orario`; `canali::invii_ultima_ora` conta gli invii `canale:<id>`. `mcp_canale::maschera_testo` sul registro; middleware che rimette nel formato di XBot gli errori in testo di `rmcp`. `main.rs` aggiunge `rmcp=warn` al filtro dei log e toglie il socket a SIGTERM; `mcp_interno::avvia` toglie un socket orfano (nessuno risponde) e non scavalca un server vivo.
- **Utilizzo e flusso** (20:10): `utilizzo::riepilogo(conn, da, a, &Filtri)` con tre filtri facoltativi nella stessa clausola `WHERE` di tutte le query. `bus::Segnale` ha `Richiesta {id, tipo, stato}` (pubblicato da `richieste::crea` e `richieste::chiudi_con`) e `Barra` (strumento `permesso` all'apertura e alla chiusura dell'attesa, catena fermata, `letto` su ogni sessione dell'assistente); `rotte/flusso.rs` li traduce in `event: richiesta` ed `event: barra`, e dopo ogni `event: turno` manda anche `barra`. Prove: `tests/s2_flussi.rs`.


## 10. Fase 2.E S-B (server: macchine, instradamento, squadra)

*2026-10-24 21:17, costruttore S-B. Migrazione 006, schema 6. Prove: `crates/server/tests/s2e_macchine.rs` (3 d'integrazione con un lavoratore finto WebSocket) e `xbot_db::lavori` (unitario); clippy `-D warnings` e fmt puliti.*

- **Schema 006**: `macchina.credenziale_hash` (sha256 della credenziale del lavoratore, indice unico), tabella `provider_macchina (provider_id, macchina_id, programma, versione, visto_ms)` (lo stesso provider su più macchine; l'id provider resta `claude_code`, `cursor_agent`, `codex`, `gemini`), `assistente.macchina_predefinita_id` e `assistente.usa_chrome`. Un provider dichiarato solo da un computer ha `provider.macchina_id` = quella macchina e il server non lo carica (`provider::attivi` prende solo `macchina_id IS NULL`); un modello con lo stesso id di un altro provider si salva come `<provider>/<id>` e va al client senza prefisso.
- **Moduli**: `xbot_db::lavori` (lavori e `lavoro_evento` con `seq`), `macchine::{collega, per_credenziale, battito_lavoratore, rimuovi}`, `provider::{registra_su_macchina, della_macchina, macchine_del_provider, macchine_per_provider}`; nel server `canale_macchina.rs` (Hub dei socket e dei lavori vivi, frame, sorvegliante del lease), `motore/lavori.rs` (turno remoto, attesa, push, «sulla VPS»), `rotte/squadra.rs`.
- **Instradamento** (`motore/preparazione.rs::scegli_provider` → `Scelta`): la macchina voluta è quella dell'invio o della sessione (che alla creazione prende `macchina_predefinita_id`); conta come remota solo se ha provider dichiarati. Auto + remota collegata → primo provider della macchina (prima `claude_code`); Auto + remota spenta → primo provider del server con avviso `ripiego_vps`; provider fisso su quella macchina → lavoro (subito o in attesa); provider fisso solo remoto senza macchina voluta → la macchina collegata che lo ha, o la prima.
- **Segreti**: viaggiano solo nel frame `lavoro` (memoria del server, TLS); `lavoro.carico_json` salva i nomi delle variabili e delle intestazioni, mai i valori, e la lunghezza del testo di sistema.
- **Lease**: battito del lavoratore → `lease_scade_ms` = ora + 60 s; senza battito oltre il lease la macchina va in `riconnessione` (lavori vivi) o `ferma`; un lavoro preso senza battito da 10 minuti fallisce `macchina_persa` (evento `macchina_persa`). Socket chiuso: il turno resta `in_corso`, il giornale del lavoratore rimanda gli eventi, il server li applica una volta per `seq`.
- **Testo di sistema**: ogni collega ha «lavora con <provider>/<modello> su <macchina>; routine: …», l'assistente riceve «Tu sei <nome>, <titolo>: <descrizione>; lavori con … su …; le tue routine: …».
- **Routine**: l'invio porta la macchina predefinita dell'assistente; vale la stessa regola (Auto ripiega, client fisso aspetta).

## 11. Fase 2.E-bis S-B (chiamata vocale, risolta in coda, allegati sul Mac)

*2026-10-24 22:22, costruttore S-B. Migrazione 007, schema 7. Prove: `s2e_macchine.rs` (`chiamata_vocale_e_voto_sul_server`, `risolta_in_coda_e_allegati_nel_lavoro`), `xbot_db::lavori` (unitario), nel lavoratore `esecuzione::test` e `tests/canale.rs::allegati_scritti_e_cancellati_a_fine_turno`. Dettagli in `CONTRATTO-2E.md` §12.*

- **Chiamata e voto**: `rotte/chiamate.rs`: la riga `evento` `chiamata_vocale` e il campo `voto` nel corpo delle righe (chiamate e risposte dell'assistente), riscritte con `righe::riscrivi` e pubblicate sul bus.
- **Risolta in coda**: tabella `risolta_in_coda` (007), `lavori::{accoda_risolta, prendi_risolte}`; `canale_macchina::{manda_risolta, consegna_risolte}` dopo i `lavoro` del `ciao`.
- **Allegati**: `motore::lavori::allegati_sul_filo` (25 MB, base64, nel carico salvato senza contenuto); nel lavoratore `esecuzione::scrivi_allegato` (0600 in `turni/<id>/allegati/`, cancellati con la cartella del turno) e `protocollo::AllegatoFilo` (oggetto o percorso).

## 12. Fase 2.H S (computer degli assistenti)

*2026-10-25 07:08, costruttore S. Migrazione 009, schema 9. Prove: `tests/s2h_computer.rs` (desktop finto `tests/desktop-finto.py`), `xbot_db::computer` e `computer::test` (unitari). Dettagli in `CONTRATTO-2E.md` §20.*

- **Moduli**: `xbot_db::computer` (volontà, stato, posti e porte), nel server `computer.rs` (avvio in ordine con attesa del socket X e delle porte, gruppi di processi, sorvegliante, spegnimento alla chiusura, riaccensione all'avvio), `rotte/computer.rs` (stato, accendi/spegni/ricrea, ponte WebSocket→x11vnc, proxy HTTP e WebSocket→ttyd), `motore/mcp.rs::computer_nel_turno` (DISPLAY, `chrome-devtools`, testo di sistema).


## 13. Fase 2.I S (OAuth dei collegamenti, account dei provider)

*2026-10-25 07:53, costruttore S. Migrazione 010, schema 10. Prove: `tests/s2i_oauth.rs` (server OAuth e MCP finto `tests/oauth-finto.py`, porta 8831), `tests/s2i_provider.rs`, unitari in `xbot_db::oauth`, `collegamenti::test`, `xbot_provider::accesso`, `codex_quota`, `protocollo_prove`. Dettagli in `CONTRATTO-2I.md` §7.*

- **Moduli**: `xbot_db::oauth` (accesso per collegamento, token cifrati con la chiave della cassaforte), `xbot_db::collegamenti::segreti` (intestazioni e ambiente: fusione del PATCH, nomi in chiaro); nel server `oauth.rs` (scoperta, registrazione, PKCE, scambio, rinnovo e revoca con `rmcp::transport::auth`), `accessi.rs` (Entra/Esci dei client, account del server ogni 5 minuti), `rotte/oauth.rs`, `rotte/provider.rs` (prima in `varie.rs`), `rotte/catalogo.rs`, `motore/oauth_turno.rs` (rinnovo prima del turno, avviso in chat); in `xbot-provider` `accesso.rs` (account e comandi di accesso dei client) e `codex_quota.rs`; nel lavoratore `accessi.rs` (frame `accedi`, `esci`, `codice_accesso` → `accesso`).
