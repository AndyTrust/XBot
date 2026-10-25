-- XBot: schema dei dati (SQLite), versione 1
-- Scritto il 2026-10-24 14:45 dalla chat master, dalla Specifica di comportamento (sez. 12 e 14).
-- Regole: tabelle STRICT, nomi in italiano, tempi in millisecondi dall'epoca (INTEGER, suffisso _ms),
-- id testuali ULID dove la riga nasce dal client (idempotenza), JSON in colonne TEXT con suffisso _json.
-- Nessun segreto in chiaro: le colonne *_cifrato sono BLOB prodotti dalla cassaforte (XChaCha20-Poly1305).
-- Da attivare a ogni connessione: PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;

-- ───────────────────────── 0. Versione dello schema ─────────────────────────
CREATE TABLE schema_meta (
  chiave  TEXT PRIMARY KEY,
  valore  TEXT NOT NULL
) STRICT;
INSERT INTO schema_meta VALUES ('schema_version', '1'), ('creato', '2026-10-24');

-- ───────────────────────── 1. Accesso (un solo utente) ─────────────────────────
CREATE TABLE utente (
  id            INTEGER PRIMARY KEY CHECK (id = 1),
  nome          TEXT NOT NULL,
  totp_cifrato  BLOB,                       -- segreto TOTP, cifrato con la chiave della cassaforte
  creato_ms     INTEGER NOT NULL
) STRICT;

CREATE TABLE accesso_web (                   -- sessioni di login della PWA e del Mac
  id               TEXT PRIMARY KEY,         -- hash del token, mai il token
  dispositivo      TEXT NOT NULL,            -- «S24», «MacBook Air»
  creato_ms        INTEGER NOT NULL,
  ultimo_uso_ms    INTEGER,
  scade_ms         INTEGER NOT NULL,
  revocato_ms      INTEGER
) STRICT;

-- ───────────────────────── 2. Provider, modelli, quote ─────────────────────────
-- Un provider è un programma lanciato dal motore (client ufficiale), mai un'API con il token dell'abbonamento.
CREATE TABLE provider (
  id              TEXT PRIMARY KEY,          -- 'claude_code' | 'grok' | 'cursor_agent'
  nome            TEXT NOT NULL,
  programma       TEXT NOT NULL,             -- percorso dell'eseguibile sulla macchina che lo ospita
  argomenti_json  TEXT NOT NULL DEFAULT '[]',
  macchina_id     TEXT REFERENCES macchina(id) ON DELETE SET NULL,  -- dove gira (NULL = la VPS)
  attivo          INTEGER NOT NULL DEFAULT 1 CHECK (attivo IN (0,1)),
  creato_ms       INTEGER NOT NULL
) STRICT;

CREATE TABLE modello (
  id                        TEXT PRIMARY KEY,   -- 'claude-opus-5-5', 'grok-4.5', ...
  provider_id               TEXT NOT NULL REFERENCES provider(id) ON DELETE CASCADE,
  etichetta                 TEXT NOT NULL,
  dettaglio                 TEXT,
  livelli_ragionamento_json TEXT NOT NULL DEFAULT '[]',  -- es. ["low","medium","high","max"]
  disponibile               INTEGER NOT NULL DEFAULT 1 CHECK (disponibile IN (0,1)),
  nascosto                  INTEGER NOT NULL DEFAULT 0 CHECK (nascosto IN (0,1)),
  ordine                    INTEGER NOT NULL DEFAULT 0
) STRICT;

-- Regola (da craft-ai-core): se il modello scelto non è disponibile, errore. Mai ripiego silenzioso.
CREATE TABLE quota_rilevazione (             -- l'anello per provider
  id                 INTEGER PRIMARY KEY,
  provider_id        TEXT NOT NULL REFERENCES provider(id) ON DELETE CASCADE,
  rilevata_ms        INTEGER NOT NULL,
  percentuale_usata  REAL NOT NULL CHECK (percentuale_usata BETWEEN 0 AND 100),
  azzera_ms          INTEGER,                -- quando la quota si rinnova
  fonte              TEXT NOT NULL CHECK (fonte IN ('client','stima')),  -- letta dal client o stimata dai token
  dettaglio_json     TEXT
) STRICT;
CREATE INDEX quota_rilevazione_provider ON quota_rilevazione(provider_id, rilevata_ms DESC);

-- ───────────────────────── 3. Assistenti, sezioni della barra, progetti ─────────────────────────
CREATE TABLE assistente (
  id                       TEXT PRIMARY KEY,   -- ULID
  nome                     TEXT NOT NULL,
  descrizione              TEXT,
  titolo                   TEXT,               -- «Caposquadra vendite»
  avatar_forma             TEXT,               -- forma dell'avatar (personaggio, iniziale o icona)
  avatar_colore            TEXT,
  avatar_percorso          TEXT,               -- foto, se c'è
  profilo_percorso         TEXT,               -- il .md del profilo (ex agente), in .claude/agents/
  memoria_cartella         TEXT,               -- cartella della memoria che legge all'apertura
  quaderno_percorso        TEXT,               -- .claude/memoria/agenti/<nome>.md
  provider_predefinito_id  TEXT REFERENCES provider(id) ON DELETE SET NULL,
  modello_predefinito_id   TEXT REFERENCES modello(id) ON DELETE SET NULL,
  livello_ragionamento     TEXT,
  voce_id                  TEXT,               -- voce fish.audio
  voce_velocita            REAL,
  voce_lingua              TEXT,
  colore_in_chat           INTEGER NOT NULL DEFAULT 1 CHECK (colore_in_chat IN (0,1)),
  fissato                  INTEGER NOT NULL DEFAULT 0 CHECK (fissato IN (0,1)),
  nascosto                 INTEGER NOT NULL DEFAULT 0 CHECK (nascosto IN (0,1)),
  ordine_fissato           INTEGER,
  creato_ms                INTEGER NOT NULL,
  aggiornato_ms            INTEGER NOT NULL,
  archiviato_ms            INTEGER
) STRICT;

CREATE TABLE sezione_barra (                 -- gruppi della barra laterale; '__non_assegnati__' è la sezione speciale
  id      TEXT PRIMARY KEY,
  nome    TEXT NOT NULL,
  ordine  INTEGER NOT NULL DEFAULT 0,
  chiusa  INTEGER NOT NULL DEFAULT 0 CHECK (chiusa IN (0,1))
) STRICT;
INSERT INTO sezione_barra VALUES ('__non_assegnati__', 'Non assegnati', 9999, 0);

CREATE TABLE sezione_assistente (
  sezione_id     TEXT NOT NULL REFERENCES sezione_barra(id) ON DELETE CASCADE,
  assistente_id  TEXT NOT NULL REFERENCES assistente(id) ON DELETE CASCADE,
  ordine         INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (sezione_id, assistente_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE progetto (                      -- gli spazi di lavoro dell'utente
  id               TEXT PRIMARY KEY,
  nome             TEXT NOT NULL,
  spazio           TEXT NOT NULL,            -- es. 'Azienda' | 'Personale' | 'Patrimonio'
  cartella         TEXT,                     -- cartella del codice
  memoria_percorso TEXT,                     -- pagina dello spazio in Memoria/
  creato_ms        INTEGER NOT NULL
) STRICT;

-- ───────────────────────── 4. Sessioni, righe di trascrizione, invii, turni ─────────────────────────
CREATE TABLE sessione (
  id                       TEXT PRIMARY KEY,   -- ULID
  assistente_id            TEXT NOT NULL REFERENCES assistente(id) ON DELETE CASCADE,
  progetto_id              TEXT REFERENCES progetto(id) ON DELETE SET NULL,
  titolo                   TEXT,
  stato                    TEXT NOT NULL DEFAULT 'attiva' CHECK (stato IN ('attiva','archiviata')),
  provider_id              TEXT REFERENCES provider(id) ON DELETE SET NULL,
  modello_id               TEXT REFERENCES modello(id) ON DELETE SET NULL,
  livello_ragionamento     TEXT,
  macchina_id              TEXT REFERENCES macchina(id) ON DELETE SET NULL,  -- dove gira ora (NULL = VPS)
  cartella_lavoro          TEXT,
  sessione_esterna_id      TEXT,               -- id della sessione nel client ufficiale (es. claude --resume)
  generazione              INTEGER NOT NULL DEFAULT 1,   -- cresce quando la cronologia si azzera
  ultimo_seq               INTEGER NOT NULL DEFAULT 0,   -- posizione dell'ultima riga
  ultimo_aggiornata_seq    INTEGER NOT NULL DEFAULT 0,   -- orologio degli aggiornamenti (cursori dei client)
  sessione_padre_id        TEXT REFERENCES sessione(id) ON DELETE SET NULL,  -- fork o thread
  riassunto_passaggio      TEXT,               -- scritto quando cambia provider o macchina
  creata_ms                INTEGER NOT NULL,
  aggiornata_ms            INTEGER NOT NULL,
  archiviata_ms            INTEGER
) STRICT;
CREATE INDEX sessione_assistente ON sessione(assistente_id, aggiornata_ms DESC);

-- La riga di trascrizione: stesso seq, updated_seq crescente = «il testo che scorre è la riga riscritta».
CREATE TABLE riga (
  sessione_id     TEXT NOT NULL REFERENCES sessione(id) ON DELETE CASCADE,
  seq             INTEGER NOT NULL,
  id              TEXT NOT NULL UNIQUE,        -- ULID stabile della riga (reazioni, risposte, widget)
  genere          TEXT NOT NULL CHECK (genere IN
                    ('messaggio','chiamata_strumento','richiesta','allegato_utente','avviso','evento',
                     'ragionamento','diff','media','tempo_turno','esito','chiamata_voce','app_mcp')),
  ruolo           TEXT CHECK (ruolo IN ('utente','assistente','sistema','collega')),  -- 'collega' = testo di un altro agente, incorniciato
  corpo_json      TEXT,                        -- il contenuto; NULL se sta in blob
  blob_hash       TEXT REFERENCES blob(hash) ON DELETE SET NULL,
  aggiornata_seq  INTEGER NOT NULL,
  soppressa       INTEGER NOT NULL DEFAULT 0 CHECK (soppressa IN (0,1)),
  creata_ms       INTEGER NOT NULL,
  aggiornata_ms   INTEGER NOT NULL,
  PRIMARY KEY (sessione_id, seq)
) STRICT, WITHOUT ROWID;
CREATE INDEX riga_aggiornata ON riga(sessione_id, aggiornata_seq);

CREATE TABLE blob (                          -- corpi grandi e allegati, sul disco della VPS
  hash          TEXT PRIMARY KEY,            -- sha256
  dimensione    INTEGER NOT NULL,
  tipo_mime     TEXT,
  percorso      TEXT NOT NULL,
  creato_ms     INTEGER NOT NULL
) STRICT;

-- L'invio: il client manda un id suo (nonce); se arriva due volte è 'duplicato'. Il diario degli invii riprova.
CREATE TABLE invio (
  id                       TEXT PRIMARY KEY,   -- nonce del client (ULID)
  sessione_id              TEXT NOT NULL REFERENCES sessione(id) ON DELETE CASCADE,
  testo                    TEXT NOT NULL,
  allegati_json            TEXT NOT NULL DEFAULT '[]',
  risposta_a_riga_id       TEXT REFERENCES riga(id) ON DELETE SET NULL,
  origine                  TEXT NOT NULL,      -- 'pwa' | 'mac' | 'voce' | 'routine:<id>' | 'canale:<id>'
  macchina_destinazione_id TEXT REFERENCES macchina(id) ON DELETE SET NULL,  -- «esegui su»
  provider_richiesto_id    TEXT REFERENCES provider(id) ON DELETE SET NULL,
  composto_ms              INTEGER,
  inviato_ms               INTEGER NOT NULL,
  stato                    TEXT NOT NULL DEFAULT 'in_coda' CHECK (stato IN
                             ('in_coda','accettato','duplicato','rifiutato','consegnato','fallito')),
  consegna                 TEXT CHECK (consegna IN
                             ('in_coda_prossimo_turno','inserito_nel_turno','trattenuto','avviato_continuazione',
                              'salvato_per_dopo','continuazione_bloccata')),
  rifiuto_codice           TEXT,
  rifiuto_testo            TEXT,
  riga_eco_id              TEXT REFERENCES riga(id) ON DELETE SET NULL,  -- la riga 'messaggio' creata dall'invio
  accettato_ms             INTEGER
) STRICT;
CREATE INDEX invio_sessione ON invio(sessione_id, inviato_ms);

-- Il turno: un giro del provider (processo lanciato) per un invio. Qui si contano token e costo.
CREATE TABLE turno (
  id                 TEXT PRIMARY KEY,
  sessione_id        TEXT NOT NULL REFERENCES sessione(id) ON DELETE CASCADE,
  invio_id           TEXT REFERENCES invio(id) ON DELETE SET NULL,
  provider_id        TEXT NOT NULL REFERENCES provider(id),
  modello_id         TEXT REFERENCES modello(id) ON DELETE SET NULL,
  livello            TEXT,
  macchina_id        TEXT REFERENCES macchina(id) ON DELETE SET NULL,
  stato              TEXT NOT NULL DEFAULT 'in_corso' CHECK (stato IN
                       ('in_corso','riuscito','fallito','annullato','interrotto')),
  processo_pid       INTEGER,
  iniziato_ms        INTEGER NOT NULL,
  finito_ms          INTEGER,
  durata_ms          INTEGER,
  token_ingresso     INTEGER,
  token_uscita       INTEGER,
  token_cache        INTEGER,
  costo_stimato_eur  REAL,
  fasi_json          TEXT,                     -- «Worked 1m 20s (fase …)»
  errore_codice      TEXT,
  errore_testo       TEXT
) STRICT;
CREATE INDEX turno_sessione ON turno(sessione_id, iniziato_ms);
CREATE INDEX turno_provider_tempo ON turno(provider_id, iniziato_ms);  -- per stimare le quote

CREATE TABLE allegato (
  id            TEXT PRIMARY KEY,
  sessione_id   TEXT NOT NULL REFERENCES sessione(id) ON DELETE CASCADE,
  riga_id       TEXT REFERENCES riga(id) ON DELETE SET NULL,
  nome          TEXT NOT NULL,
  blob_hash     TEXT NOT NULL REFERENCES blob(hash),
  caricato_ms   INTEGER NOT NULL
) STRICT;

-- Stato di lettura per assistente: non letti, in attesa dell'utente (badge), notifiche.
CREATE TABLE stato_lettura (
  assistente_id            TEXT PRIMARY KEY REFERENCES assistente(id) ON DELETE CASCADE,
  letto_fino_seq           INTEGER NOT NULL DEFAULT 0,
  sessione_corrente_id     TEXT REFERENCES sessione(id) ON DELETE SET NULL,
  in_attesa_utente         INTEGER NOT NULL DEFAULT 0 CHECK (in_attesa_utente IN (0,1)),
  al_lavoro                INTEGER NOT NULL DEFAULT 0 CHECK (al_lavoro IN (0,1)),
  notifiche                INTEGER NOT NULL DEFAULT 1 CHECK (notifiche IN (0,1)),
  notifica_aggiornamenti   INTEGER NOT NULL DEFAULT 0 CHECK (notifica_aggiornamenti IN (0,1)),
  ultima_vista_ms          INTEGER
) STRICT, WITHOUT ROWID;

-- ───────────────────────── 5. Richieste all'utente (permessi, moduli, segreti, domande) ─────────────────────────
CREATE TABLE richiesta (
  id                 TEXT PRIMARY KEY,
  sessione_id        TEXT NOT NULL REFERENCES sessione(id) ON DELETE CASCADE,
  riga_id            TEXT REFERENCES riga(id) ON DELETE SET NULL,
  tipo               TEXT NOT NULL CHECK (tipo IN
                       ('permesso_locale','revisione_comando','modulo','segreto','cassaforte','domanda',
                        'collegamento','spostamento_sessione')),
  carico_json        TEXT NOT NULL,            -- cosa si chiede (comando, campi del modulo, voce richiesta)
  stato              TEXT NOT NULL DEFAULT 'aperta' CHECK (stato IN
                       ('aperta','approvata','negata','scaduta','ritirata')),
  creata_ms          INTEGER NOT NULL,
  scade_ms           INTEGER,
  risolta_ms         INTEGER,
  risoluzione_json   TEXT,                     -- valori del modulo, 'una_volta' | 'sempre', comando approvato
  piattaforma        TEXT CHECK (piattaforma IN ('pwa','mac','voce','canale'))
) STRICT;
CREATE INDEX richiesta_aperte ON richiesta(stato, creata_ms) WHERE stato = 'aperta';

-- ───────────────────────── 6. Macchine (computer collegati) e lavori ─────────────────────────
-- Identità con chiave P-256, richieste firmate, attesa lunga, generazione e recinto.
CREATE TABLE macchina (
  id                    TEXT PRIMARY KEY,      -- id del dispositivo
  nome                  TEXT NOT NULL,         -- 'vps', 'mac', 's24'
  etichetta             TEXT,
  genere                TEXT NOT NULL CHECK (genere IN ('vps','mac','android','ios','windows')),
  chiave_pubblica       TEXT NOT NULL,         -- P-256, SPKI base64
  os                    TEXT,
  arch                  TEXT,
  versione_lavoratore   TEXT,
  posti                 INTEGER NOT NULL DEFAULT 1 CHECK (posti BETWEEN 1 AND 4),  -- sessioni contemporanee
  stato                 TEXT NOT NULL DEFAULT 'ferma' CHECK (stato IN
                          ('connessa','avvio','riconnessione','ferma','rimossa')),
  permesso_esecuzione   TEXT NOT NULL DEFAULT 'ask' CHECK (permesso_esecuzione IN ('always','ask','never')),
  tieni_sveglia         INTEGER NOT NULL DEFAULT 0 CHECK (tieni_sveglia IN (0,1)),
  capacita_json         TEXT NOT NULL DEFAULT '[]',   -- es. ["exec.run","fs.read","browser.use","voce.microfono"]
  ultimo_battito_ms     INTEGER,
  registrata_ms         INTEGER NOT NULL,
  rimossa_ms            INTEGER
) STRICT;

CREATE TABLE radice (                        -- cartelle concesse su una macchina
  id                 TEXT PRIMARY KEY,
  macchina_id        TEXT NOT NULL REFERENCES macchina(id) ON DELETE CASCADE,
  etichetta          TEXT NOT NULL,
  percorso           TEXT NOT NULL,
  permessi_json      TEXT NOT NULL DEFAULT '["read"]',  -- read, write, execute, discover_environment, expose_mcp
  UNIQUE (macchina_id, percorso)
) STRICT;

CREATE TABLE sfida_dispositivo (             -- anti-ripetizione delle richieste firmate
  nonce        TEXT PRIMARY KEY,
  macchina_id  TEXT NOT NULL REFERENCES macchina(id) ON DELETE CASCADE,
  emessa_ms    INTEGER NOT NULL,
  scade_ms     INTEGER NOT NULL,
  usata_ms     INTEGER
) STRICT;

CREATE TABLE lavoro (                        -- un'operazione da eseguire su una macchina
  id              TEXT PRIMARY KEY,
  macchina_id     TEXT NOT NULL REFERENCES macchina(id) ON DELETE CASCADE,
  sessione_id     TEXT REFERENCES sessione(id) ON DELETE SET NULL,
  turno_id        TEXT REFERENCES turno(id) ON DELETE SET NULL,
  capacita        TEXT NOT NULL,              -- 'exec.run' | 'fs.read' | 'fs.write' | 'sessione.apri' | 'browser.use' | ...
  carico_json     TEXT NOT NULL,
  autorita        TEXT NOT NULL DEFAULT 'nessuna' CHECK (autorita IN ('nessuna','stabile','approvazione')),
  richiesta_id    TEXT REFERENCES richiesta(id) ON DELETE SET NULL,
  generazione     INTEGER NOT NULL DEFAULT 1, -- il lavoratore rifiuta generazioni vecchie
  recinto_id      TEXT,                       -- fence: un solo lavoratore per sessione
  tentativo       INTEGER NOT NULL DEFAULT 1,
  lease_scade_ms  INTEGER,
  stato           TEXT NOT NULL DEFAULT 'in_coda' CHECK (stato IN
                    ('in_coda','preso','in_corso','riuscito','fallito','annullato','scaduto')),
  creato_ms       INTEGER NOT NULL,
  preso_ms        INTEGER,
  finito_ms       INTEGER,
  codice_uscita   INTEGER,
  esito_json      TEXT                        -- stdout/stderr finali o riferimento a blob; il socket chiuso non annulla il lavoro
) STRICT;
CREATE INDEX lavoro_coda ON lavoro(macchina_id, stato, creato_ms);

CREATE TABLE lavoro_evento (                 -- progresso a flusso (stdout, stderr, stato)
  lavoro_id  TEXT NOT NULL REFERENCES lavoro(id) ON DELETE CASCADE,
  seq        INTEGER NOT NULL,
  tipo       TEXT NOT NULL,                   -- 'stdout' | 'stderr' | 'stato' | 'cwd'
  dati       TEXT NOT NULL,
  ms         INTEGER NOT NULL,
  PRIMARY KEY (lavoro_id, seq)
) STRICT, WITHOUT ROWID;

-- ───────────────────────── 7. Permessi per azione e regole di revisione ─────────────────────────
-- always|ask|never per azione, concessioni stabili per destinatario, regole in linguaggio naturale.
CREATE TABLE permesso_azione (
  id             TEXT PRIMARY KEY,
  macchina_id    TEXT REFERENCES macchina(id) ON DELETE CASCADE,        -- NULL = tutte
  assistente_id  TEXT REFERENCES assistente(id) ON DELETE CASCADE,      -- NULL = tutti
  azione         TEXT NOT NULL,               -- 'run-command' | 'read-file' | 'write-file' | 'send-message' | 'use-app' | 'phone-tool' | 'strumento_mcp'
  destinatario   TEXT,                        -- app, destinatario, server MCP (ambito 'soggetto')
  risorsa        TEXT,                        -- percorso o comando (ambito 'risorsa')
  concesso       TEXT NOT NULL CHECK (concesso IN ('sempre','mai')),
  concesso_ms    INTEGER NOT NULL,
  ritirato_ms    INTEGER
) STRICT;
CREATE INDEX permesso_azione_cerca ON permesso_azione(azione, destinatario, risorsa) WHERE ritirato_ms IS NULL;

CREATE TABLE regola_revisione (              -- Auto-review: massimo 20 regole, 1000 caratteri (controllo nell'app)
  id             TEXT PRIMARY KEY,
  assistente_id  TEXT REFERENCES assistente(id) ON DELETE CASCADE,      -- NULL = globale
  tipo           TEXT NOT NULL CHECK (tipo IN ('consenti','blocca')),
  testo          TEXT NOT NULL CHECK (length(testo) <= 1000),
  ordine         INTEGER NOT NULL DEFAULT 0,
  attiva         INTEGER NOT NULL DEFAULT 1 CHECK (attiva IN (0,1)),
  creata_ms      INTEGER NOT NULL
) STRICT;

-- ───────────────────────── 8. Cassaforte ─────────────────────────
-- Tutto il sensibile sta in campi_cifrati (JSON cifrato). In chiaro solo il riferimento: titolo, siti, ultime 4 cifre.
CREATE TABLE cassaforte_chiave (             -- parametri della chiave principale (Argon2id); la passphrase non si salva
  id               INTEGER PRIMARY KEY CHECK (id = 1),
  kdf              TEXT NOT NULL,              -- 'argon2id'
  sale             BLOB NOT NULL,
  parametri_json   TEXT NOT NULL,              -- m, t, p
  verifica         BLOB NOT NULL,              -- tag per riconoscere la passphrase giusta
  creata_ms        INTEGER NOT NULL,
  ruotata_ms       INTEGER
) STRICT;

CREATE TABLE cassaforte_voce (
  id               TEXT PRIMARY KEY,
  genere           TEXT NOT NULL CHECK (genere IN ('accesso','carta','contatto','segreto')),
  titolo           TEXT NOT NULL,
  siti_json        TEXT NOT NULL DEFAULT '[]',   -- accesso: siti (https); segreto: nome della variabile in riferimento_json
  riferimento_json TEXT NOT NULL DEFAULT '{}',   -- in chiaro, non sensibile: nome utente, brand, last4, exp_month, exp_year, holder, nickname, nome contatto
  campi_cifrati    BLOB NOT NULL,                -- accesso: password, note; carta: numero, cvc, indirizzo; contatto: telefono, email, indirizzo; segreto: valore
  origine          TEXT,                         -- 'manuale' | 'csv:bitwarden' | 'csv:1password' | ...
  creata_ms        INTEGER NOT NULL,
  aggiornata_ms    INTEGER NOT NULL,
  ultimo_uso_ms    INTEGER
) STRICT;
CREATE INDEX cassaforte_voce_genere ON cassaforte_voce(genere, titolo);

CREATE TABLE cassaforte_uso (                -- chi ha usato cosa: l'assistente non vede mai il valore in chat
  id             INTEGER PRIMARY KEY,
  voce_id        TEXT NOT NULL REFERENCES cassaforte_voce(id) ON DELETE CASCADE,
  sessione_id    TEXT REFERENCES sessione(id) ON DELETE SET NULL,
  assistente_id  TEXT REFERENCES assistente(id) ON DELETE SET NULL,
  azione         TEXT NOT NULL CHECK (azione IN ('elenco','compila','riferimento','variabile_ambiente')),
  richiesta_id   TEXT REFERENCES richiesta(id) ON DELETE SET NULL,
  ms             INTEGER NOT NULL
) STRICT;

CREATE TABLE segreto_assistente (            -- quali segreti (genere 'segreto') passano a quale assistente come variabili
  voce_id        TEXT NOT NULL REFERENCES cassaforte_voce(id) ON DELETE CASCADE,
  assistente_id  TEXT NOT NULL REFERENCES assistente(id) ON DELETE CASCADE,
  PRIMARY KEY (voce_id, assistente_id)
) STRICT, WITHOUT ROWID;

-- ───────────────────────── 9. Collegamenti (server MCP) e canali (client esterni) ─────────────────────────
CREATE TABLE collegamento (                  -- «Connetti app»: un collegamento è un server MCP
  id                       TEXT PRIMARY KEY,
  nome                     TEXT NOT NULL,
  trasporto                TEXT NOT NULL CHECK (trasporto IN ('stdio','http')),
  comando                  TEXT,
  argomenti_json           TEXT NOT NULL DEFAULT '[]',
  url                      TEXT,
  intestazioni_cifrate     BLOB,              -- Authorization e altre
  ambiente_cifrato         BLOB,
  macchina_id              TEXT REFERENCES macchina(id) ON DELETE SET NULL,  -- dove gira (stdio)
  categoria                TEXT,
  logo_percorso            TEXT,
  istruzioni               TEXT,              -- istruzioni personalizzate per gli assistenti
  abilitato                INTEGER NOT NULL DEFAULT 1 CHECK (abilitato IN (0,1)),
  creato_ms                INTEGER NOT NULL,
  ultimo_ok_ms             INTEGER,
  ultimo_errore            TEXT
) STRICT;

CREATE TABLE collegamento_assistente (
  collegamento_id  TEXT NOT NULL REFERENCES collegamento(id) ON DELETE CASCADE,
  assistente_id    TEXT NOT NULL REFERENCES assistente(id) ON DELETE CASCADE,
  abilitato        INTEGER NOT NULL DEFAULT 1 CHECK (abilitato IN (0,1)),
  PRIMARY KEY (collegamento_id, assistente_id)
) STRICT, WITHOUT ROWID;

CREATE TABLE permesso_strumento (            -- per strumento: sempre | chiedi | bloccato; '*lettura' e '*scrittura' come predefiniti
  collegamento_id  TEXT NOT NULL REFERENCES collegamento(id) ON DELETE CASCADE,
  strumento        TEXT NOT NULL,
  permesso         TEXT NOT NULL CHECK (permesso IN ('sempre','chiedi','bloccato')),
  PRIMARY KEY (collegamento_id, strumento)
) STRICT, WITHOUT ROWID;

CREATE TABLE oauth_account (                 -- accessi OAuth dei collegamenti
  id               TEXT PRIMARY KEY,
  collegamento_id  TEXT NOT NULL REFERENCES collegamento(id) ON DELETE CASCADE,
  account          TEXT NOT NULL,             -- email o nome dell'account
  token_cifrato    BLOB NOT NULL,
  scade_ms         INTEGER,
  aggiornato_ms    INTEGER NOT NULL
) STRICT;

CREATE TABLE canale (                        -- client esterni che usano XBot via MCP
  id              TEXT PRIMARY KEY,
  nome            TEXT NOT NULL,              -- 'grok-bot' | 'reason' | ...
  token_hash      TEXT NOT NULL UNIQUE,       -- mai il token
  assistente_id   TEXT REFERENCES assistente(id) ON DELETE SET NULL,  -- chi risponde alle sue richieste
  strumenti_json  TEXT NOT NULL DEFAULT '[]', -- strumenti concessi
  creato_ms       INTEGER NOT NULL,
  ultimo_uso_ms   INTEGER,
  revocato_ms     INTEGER
) STRICT;

CREATE TABLE canale_chiamata (               -- registro di cosa chiedono i canali (argomenti già privati di segreti)
  id             INTEGER PRIMARY KEY,
  canale_id      TEXT NOT NULL REFERENCES canale(id) ON DELETE CASCADE,
  strumento      TEXT NOT NULL,
  argomenti_json TEXT,
  esito          TEXT NOT NULL CHECK (esito IN ('ok','errore','negato','in_attesa')),
  richiesta_id   TEXT REFERENCES richiesta(id) ON DELETE SET NULL,
  ms             INTEGER NOT NULL,
  durata_ms      INTEGER
) STRICT;

-- ───────────────────────── 10. Routine (automazioni) ─────────────────────────
CREATE TABLE routine (
  id                TEXT PRIMARY KEY,
  assistente_id     TEXT NOT NULL REFERENCES assistente(id) ON DELETE CASCADE,
  nome              TEXT NOT NULL,
  descrizione       TEXT,
  corpo             TEXT NOT NULL,            -- il compito da dare
  attivatore_tipo   TEXT NOT NULL CHECK (attivatore_tipo IN ('cron','ogni_secondi','webhook','evento')),
  attivatore_json   TEXT NOT NULL,            -- {"expression":"30 7 * * *"} | {"every_seconds":3600} | {"evento":"posta_nuova"}
  abilitata         INTEGER NOT NULL DEFAULT 1 CHECK (abilitata IN (0,1)),
  prossima_ms       INTEGER,
  ultima_ms         INTEGER,
  ultimo_esito      TEXT,
  creata_ms         INTEGER NOT NULL
) STRICT;

CREATE TABLE routine_esecuzione (
  id            TEXT PRIMARY KEY,
  routine_id    TEXT NOT NULL REFERENCES routine(id) ON DELETE CASCADE,
  sessione_id   TEXT REFERENCES sessione(id) ON DELETE SET NULL,
  iniziata_ms   INTEGER NOT NULL,
  finita_ms     INTEGER,
  esito         TEXT
) STRICT;

-- ───────────────────────── 11. Memoria (puntatori: la memoria vera è la cartella di note dell'utente) ─────────────────────────
CREATE TABLE memoria_scrittura (             -- cosa XBot ha scritto nel vault e quando (verifica e ripresa)
  id             INTEGER PRIMARY KEY,
  sessione_id    TEXT REFERENCES sessione(id) ON DELETE SET NULL,
  assistente_id  TEXT REFERENCES assistente(id) ON DELETE SET NULL,
  tipo           TEXT NOT NULL CHECK (tipo IN ('riassunto_sessione','quaderno','diario','nota','casella')),
  percorso       TEXT NOT NULL,
  ms             INTEGER NOT NULL
) STRICT;

-- ───────────────────────── 12. Voce ─────────────────────────
CREATE TABLE voce_chiamata (                 -- mani libere
  id             TEXT PRIMARY KEY,
  sessione_id    TEXT NOT NULL REFERENCES sessione(id) ON DELETE CASCADE,
  iniziata_ms    INTEGER NOT NULL,
  finita_ms      INTEGER,
  lingua         TEXT,
  voce_id        TEXT,
  interruzioni   INTEGER NOT NULL DEFAULT 0,
  righe          INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE TABLE dettato (                       -- premi e parla
  id             TEXT PRIMARY KEY,
  sessione_id    TEXT REFERENCES sessione(id) ON DELETE SET NULL,
  blob_hash      TEXT REFERENCES blob(hash) ON DELETE SET NULL,
  durata_ms      INTEGER,
  testo          TEXT,
  motore         TEXT NOT NULL DEFAULT 'whisper',
  ms             INTEGER NOT NULL
) STRICT;

-- ───────────────────────── 13. Impostazioni, notifiche, push ─────────────────────────
CREATE TABLE impostazione (                  -- tema, accento, lingua, fuso, suono notifiche, microfono, lingua voce
  chiave         TEXT PRIMARY KEY,
  valore_json    TEXT NOT NULL,
  aggiornata_ms  INTEGER NOT NULL
) STRICT;

CREATE TABLE notifica (
  id             TEXT PRIMARY KEY,
  assistente_id  TEXT REFERENCES assistente(id) ON DELETE CASCADE,
  sessione_id    TEXT REFERENCES sessione(id) ON DELETE CASCADE,
  titolo         TEXT NOT NULL CHECK (length(titolo) <= 200),
  corpo          TEXT NOT NULL CHECK (length(corpo) <= 140),
  percorso       TEXT,                        -- dove porta il tocco
  silenziosa     INTEGER NOT NULL DEFAULT 0 CHECK (silenziosa IN (0,1)),
  gruppo         TEXT,
  creata_ms      INTEGER NOT NULL,
  consegnata_ms  INTEGER,
  letta_ms       INTEGER
) STRICT;

CREATE TABLE abbonamento_push (              -- Web Push della PWA
  id             TEXT PRIMARY KEY,
  endpoint       TEXT NOT NULL UNIQUE,
  chiavi_json    TEXT NOT NULL,
  dispositivo    TEXT,
  creato_ms      INTEGER NOT NULL,
  ultimo_ok_ms   INTEGER
) STRICT;

-- ───────────────────────── 14. Diario di sistema ─────────────────────────
CREATE TABLE evento_sistema (
  id            INTEGER PRIMARY KEY,
  tipo          TEXT NOT NULL,                -- 'avvio' | 'provider.errore' | 'macchina.connessa' | 'quota.esaurita' | ...
  dettaglio_json TEXT,
  ms            INTEGER NOT NULL
) STRICT;
CREATE INDEX evento_sistema_tempo ON evento_sistema(ms);

-- ───────────────────────── Viste di comodo ─────────────────────────
CREATE VIEW v_barra_laterale AS               -- quello che la barra laterale mostra
SELECT sb.id AS sezione_id, sb.nome AS sezione, sb.ordine AS sezione_ordine, sb.chiusa,
       a.id AS assistente_id, a.nome, a.avatar_forma, a.avatar_colore, a.fissato,
       COALESCE(sl.in_attesa_utente, 0) AS in_attesa_utente,
       COALESCE(sl.al_lavoro, 0) AS al_lavoro,
       (SELECT COUNT(*) FROM riga r JOIN sessione s ON s.id = r.sessione_id
         WHERE s.assistente_id = a.id AND r.seq > COALESCE(sl.letto_fino_seq, 0)
           AND r.ruolo = 'assistente' AND r.genere = 'messaggio') AS non_letti
FROM assistente a
JOIN sezione_assistente sa ON sa.assistente_id = a.id
JOIN sezione_barra sb ON sb.id = sa.sezione_id
LEFT JOIN stato_lettura sl ON sl.assistente_id = a.id
WHERE a.archiviato_ms IS NULL AND a.nascosto = 0;

CREATE VIEW v_quota_corrente AS               -- l'anello per provider: ultima rilevazione
SELECT p.id AS provider_id, p.nome, q.percentuale_usata, q.azzera_ms, q.fonte, q.rilevata_ms
FROM provider p
LEFT JOIN quota_rilevazione q ON q.id = (
  SELECT id FROM quota_rilevazione WHERE provider_id = p.id ORDER BY rilevata_ms DESC LIMIT 1)
WHERE p.attivo = 1;
