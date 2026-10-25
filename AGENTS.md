# XBot: guida per gli agenti che lavorano sul codice

Unica guida del progetto. La leggono Claude Code, Cursor, Codex, Gemini e ogni altro agente che
modifica il codice; `CLAUDE.md` e `GEMINI.md`, se ci sono, la importano. Per installare XBot (non
per modificarlo) la guida è `INSTALLAZIONE-LLM.md`.

## Cos'è

XBot è un server personale in Rust con un'interfaccia web installabile (PWA). L'utente parla con i
suoi assistenti; ogni assistente lavora con il client ufficiale di un abbonamento (Claude Code,
Cursor, Grok, Codex, Gemini) lanciato come processo. Il server tiene chat, memoria, cassaforte
cifrata, collegamenti (server MCP e OAuth), routine, voce e i computer degli assistenti. Altri
computer si collegano al server con il lavoratore `xbot-worker` e vi eseguono i turni.

## Dove stanno le cose

| Cartella o file | Cosa c'è |
|---|---|
| `crates/core` | tipi, errori, `SegretoTesto` |
| `crates/db` | SQLite: `schema.sql` (versione 1) e migrazioni in `crates/db/migrazioni/`, repository, cassaforte |
| `crates/provider` | trait `Provider` e adattatori (`finto`, `claude_code`, `cursor_agent`, `codex`, `gemini`, `grok`); `prove/claude-finto` è il `claude` finto delle prove |
| `crates/server` | axum: accesso, API v1, SSE, motore dei turni, canali MCP, voce, computer degli assistenti; test in `tests/` |
| `crates/worker` | `xbot-worker`, il lavoratore che collega un computer al server |
| `web/` | interfaccia: HTML, CSS e JavaScript in moduli ES, senza framework, senza dipendenze, senza build |
| `web/vendor/novnc` | noVNC (MPL-2.0) e pako (MIT, Zlib): codice di terzi, vedi `AVVISI-TERZI.md` |
| `voce/` | `trascrivi.py` (faster-whisper), `normalizza.py`, `sigle.json` |
| `ops/` | modelli di servizio (`launchd/`, `systemd/`), blocco Caddy (`caddy/`), server Google MCP (`google-mcp/`), script di pubblicazione |
| `installa.sh`, `xbot.env.esempio` | installazione e file d'ambiente con tutte le variabili |
| `ARCHITETTURA.md` | architettura del codice: chi costruisce la segue |

Gli artefatti di compilazione non stanno nel repository: `target/` è in `.gitignore`. Con più
agenti sulla stessa copia, ognuno usa un suo `CARGO_TARGET_DIR`.

## Regole del codice

- Rust edition 2024, toolchain stable 1.99. Workspace con `resolver = "3"`.
- `unsafe_code = "forbid"`. In ogni `lib.rs`: `#![deny(clippy::unwrap_used, clippy::expect_used,
  clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`. Nei test `unwrap` è
  ammesso (`clippy.toml`).
- `cargo clippy --all-targets -- -D warnings` e `cargo fmt --check` puliti, `cargo test` verde,
  niente `dbg!`.
- File sotto 500 righe (700 al massimo). Tipi, funzioni, moduli, rotte e tabelle hanno nomi in
  italiano, come lo schema. I crate esterni si chiamano come si chiamano.
- Nessun segreto nel codice, nei log o nei file del repository. I valori sensibili passano per
  `SegretoTesto` (niente `Debug` in chiaro, niente `Serialize`).
- Tempi in millisecondi dall'epoca, `i64`. Id testuali ULID dove lo schema dice `TEXT PRIMARY KEY`.
- Log con `tracing`, livello da `XBOT_LOG`. Mai il testo dei messaggi nei log: solo id e lunghezze.
- I processi figli non ereditano le variabili `XBOT_*` del server: per ciò che l'utente o
  l'assistente vedono (terminale, client) si usa `env_clear` e si passa solo il necessario.
- Interfaccia: niente framework, niente dipendenze, niente passo di build. Tutti i riferimenti
  sono assoluti in `/app/…`. Il nome visibile sta in una sola costante (`web/nome.js`, ripetuta
  in `sw.js`).

## Regole del progetto

- I provider sono i client ufficiali come processi. XBot non chiama le API dei fornitori con il
  token dell'abbonamento, non legge le credenziali OAuth di altri client, non manda password in
  chiaro a nessun server, non lancia i comandi di accesso dei client per conto dell'utente.
- `XBOT_SEGRETO_COOKIE` cifra anche il seme TOTP: si ruota solo con `xbot-server ruota-segreto`,
  mai cambiando il file d'ambiente a mano. Un codice TOTP vale una volta per finestra di 30
  secondi: fra due prove di accesso si aspetta la finestra successiva.
- `XBOT_WEB` è un percorso assoluto, fra virgolette se contiene spazi. `XBOT_VOCE_CARTELLA`
  punta alla cartella `voce/`, non alla radice.
- Il database delle prove sta fuori dal repository e fuori dalle cartelle sincronizzate.
- Con più agenti nello stesso repository: `git add` dei soli file propri, mai `git add -A`;
  target-dir separati. Per compilare mentre altri lavorano: `git archive HEAD | tar -x -C <cartella>`.
- Nelle prove SSE si cerca il frame `event: turno` (il testo `"stato":"riuscito"` compare anche
  nelle righe degli strumenti).
- Gli orari scritti nei file vengono da `date`, mai stimati.
- Codice di terzi: si aggiunge solo con licenza compatibile e con la riga in `AVVISI-TERZI.md`.

## Compilare e provare

```bash
cargo build
cargo test                                   # db in memoria, provider finto
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Prove mirate, per esempio: `cargo test -p xbot-server --test s2_mcp --test s2_voce`.

### Impianto e avvio a mano

```bash
BIN=target/debug/xbot-server
export XBOT_DB=/tmp/xbot-prova/xbot.sqlite
XBOT_PASSWORD='…' $BIN impianta --nome Utente --totp --totp-file /tmp/xbot-prova/totp.txt
XBOT_WEB="$PWD/web" $BIN                    # ascolta su 127.0.0.1:8787
```

`impianta` crea (o rifà) l'utente: password da `XBOT_PASSWORD` (almeno 8 caratteri), `--totp`
per il secondo fattore (con `--totp-file` l'URL `otpauth://` va in un file 0600 invece che sullo
schermo), `--claude <percorso>` per il client (altrimenti dal PATH), `--finto` per il provider
finto. Altri sottocomandi: `vapid` (stampa le chiavi delle notifiche push), `ruota-segreto`.

### Giro a mano con curl

```bash
U=http://127.0.0.1:8787/api/v1
curl -s $U/salute
curl -s -c c.txt -H 'content-type: application/json' \
  -d '{"password":"…","codice_totp":"123456","dispositivo":"prova"}' $U/accesso
curl -s -b c.txt -H 'content-type: application/json' -d '{"nome":"Assistente"}' $U/assistenti
curl -s -b c.txt -H 'content-type: application/json' -d '{}' $U/assistenti/<A>/sessioni
curl -s -b c.txt -H 'content-type: application/json' \
  -d '{"id":"<ULID del client>","testo":"ciao"}' $U/sessioni/<S>/invii
curl -s -N -b c.txt "$U/sessioni/<S>/flusso"
```

Il provider `finto` risponde con il testo rovesciato in tre pezzi; con `[fallisci]` nel testo
fallisce. Il `claude` finto (`crates/provider/prove/claude-finto`, Python 3) ristampa un flusso
registrato; parole chiave `[fallisci]`, `[strumento]`, `[lento]`, `[muto]`, `[crolla]`.

### Interfaccia senza motore

Il finto server dell'API per provare `web/` senza il motore (`web/prove/`) non è ancora nel
repository pubblico: arriva in una versione successiva, ripulito dai dati di prova.

## Il motore in breve

- Accesso: password e TOTP, cookie di sessione; con «non chiedere più il codice» un cookie di
  dispositivo fidato. 5 tentativi falliti in 5 minuti per indirizzo danno 429 `troppi_tentativi`.
  Dietro un proxy: `XBOT_COOKIE_SICURO=1` e `XBOT_PROXY_FIDATO` con l'indirizzo del proxy.
- Turni: il motore carica i provider della tabella `provider` con `attivo = 1`. `claude_code`
  lancia `claude -p --output-format stream-json --input-format stream-json --verbose
  --include-partial-messages --strict-mcp-config` con `--resume`, `--model`, `--effort` e
  `--append-system-prompt` quando servono. I processi restano caldi per `XBOT_CALDO_MINUTI`.
  La quota arriva dal client a ogni turno (`GET /quote`).
- Schema: `schema.sql` è la versione 1; all'apertura `xbot-db` applica le migrazioni in ordine,
  ognuna in un savepoint. La versione corrente si legge da `GET /api/v1/salute`.
- Errori dell'API: codici testuali stabili (`provider_non_trovato`, `assistente_archiviato`,
  `testo_troppo_lungo`, `corpo_troppo_grande`, `cassaforte_chiusa`, `richiesta_chiusa`…).
- MCP interno: ogni turno di Claude Code riceve il server MCP di XBot (socket `XBOT_SOCKET`) con
  gli strumenti per assistenti, lavagna, cassaforte, collegamenti, sessioni figlie e permessi.
- Canali MCP: client esterni si collegano a `/mcp` con un token Bearer e gli strumenti concessi.
- Computer collegati: `xbot-worker enroll <url> <biglietto>` (biglietto da
  `POST /api/v1/macchine/biglietto`, vale 15 minuti) e poi `xbot-worker run` sul canale
  WebSocket `GET /api/v1/macchine/{id}/canale`. Un turno di un provider che sta su quel computer
  diventa un lavoro spedito lì.
- Computer degli assistenti (Linux): Xvfb, openbox o fluxbox, x11vnc e Chrome per assistente, tutti
  su 127.0.0.1 e figli del server; schermo con noVNC via il proxy del server, terminale con ttyd e
  tmux.
- Voce: `/trascrivi` lancia `voce/trascrivi.py` con il python di `XBOT_VOCE_PYTHON` (faster-whisper,
  serve ffmpeg); `/sintesi` usa fish.audio con `FISH_API_KEY`.
- Notifiche push: chiavi VAPID da `xbot-server vapid`.

Il dettaglio per modulo sta in `ARCHITETTURA.md`. Le variabili d'ambiente sono tutte in
`xbot.env.esempio`, con il valore predefinito.

## Come si scrive (codice, commenti, commit, documenti)

- Italiano. Frase corta, verbo «è», fatto verificabile.
- Niente «non solo… ma anche», «non è X, è Y», «funge da», «momento cruciale», «vibrante».
- Niente elenchi di tre aggettivi per riempire, niente riassunto finale, niente grassetto sparso,
  niente emoji.
- I messaggi di commit dicono cosa cambia e perché, in una riga, più il dettaglio se serve.
- Un compito che si ripete diventa uno script, non un paragrafo di istruzioni.
- Ogni affermazione «funziona» poggia su un comando lanciato e sulla sua uscita.
