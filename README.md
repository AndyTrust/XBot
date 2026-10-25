# XBot

XBot è un server personale per parlare con i propri assistenti da un'unica app, sul computer o sul
telefono. Ogni assistente lavora con il client ufficiale di un abbonamento che hai già: Claude
Code, Cursor, Grok. Il server tiene le chat, la memoria, una cassaforte cifrata per i segreti, i
collegamenti ai servizi esterni, le routine e la voce. Il motore è in Rust, l'interfaccia è una PWA
senza dipendenze. Altri computer si collegano al server e ne eseguono i lavori.

## Requisiti

- macOS o Linux.
- git, un compilatore C, python3, curl.
- Rust 1.99 o successivo (l'installatore propone rustup se manca).
- Almeno un client di un abbonamento: `claude` (Claude Code), `agent` (Cursor), `grok`.
- Facoltativi: ffmpeg per la voce; su un server Linux, Caddy per HTTPS e Xvfb, openbox, x11vnc,
  ttyd, tmux e Chrome per i computer degli assistenti.

## Installazione

```bash
git clone https://github.com/AndyTrust/XBot.git
cd XBot
./installa.sh
```

Lo script chiede il nome del bot orchestratore (predefinito XBot) e il modo, compila, crea utente,
password e secondo fattore (TOTP), installa il servizio e alla fine stampa l'indirizzo da aprire.
Rilanciarlo aggiorna programmi e interfaccia; database, segreti e utente restano.

Se l'installazione la fa un agente (Claude Code, Cursor, Codex, Gemini CLI), digli di leggere
[INSTALLAZIONE-LLM.md](INSTALLAZIONE-LLM.md): contiene cosa chiedere, cosa lanciare e cosa non
fare. Tutte le opzioni: `./installa.sh --aiuto`.

## I due modi

| Modo | Dove gira | Servizio | Indirizzo |
|---|---|---|---|
| `locale` | il tuo Mac o PC, solo per te | LaunchAgent (macOS) o systemd utente (Linux) | `http://127.0.0.1:8787/app/` |
| `vps` | un server Linux con systemd | unità di sistema, utente `xbot`, dati in `/var/lib/xbot` | `https://<tuo dominio>/app/` dietro Caddy |

Con il modo `vps` e un dominio lo script scrive il blocco Caddy e lo stampa: lo aggiungi tu al
Caddyfile. Un terzo modo, `worker`, collega un altro computer a un server esistente:
`./installa.sh --biglietto-nuovo` sul server, poi
`./installa.sh --modo worker --url <indirizzo> --biglietto <biglietto>` sull'altro computer.

Le impostazioni stanno in un file d'ambiente (`~/.config/xbot/xbot.env` oppure
`/etc/xbot/xbot.env`). Tutte le variabili, con i predefiniti, sono in
[xbot.env.esempio](xbot.env.esempio).

## Aggiornare

```bash
./installa.sh --versione            # versione in opera e ultima pubblicata
./installa.sh --aggiorna            # passa all'ultima versione (tag v*); --ramo main per seguire il ramo
./installa.sh --aggiornamento auto  # ogni giorno alle 04:00; avvisa (predefinito) o no
```

L'aggiornamento compila la versione nuova, tiene una copia dei programmi e del database di prima,
riavvia il servizio e controlla che risponda entro 30 secondi. Se la compilazione o il controllo
falliscono, rimette la versione di prima e lo dice. Ogni tentativo scrive una riga in
`<dati>/aggiornamenti.log`. Con il modo `avvisa` l'app mostra quando c'è una versione nuova.

Per tornare a una versione precedente: `git checkout <tag>` nel clone e `./installa.sh`. Se la
versione vecchia non parte con il database già migrato, rimetti la copia in `<dati>/backup/`.

## Provider supportati

| Provider | Client | Note |
|---|---|---|
| Claude Code | `claude` | registrato all'installazione se è nel PATH |
| Cursor | `agent` | tutti i modelli del tuo piano Cursor |
| Grok | `grok` | CLI di xAI |
| Codex, Gemini | `codex`, `gemini` | dal lavoratore, se installati |

XBot lancia i client come processi con il tuo accesso. L'accesso a ogni client lo fai tu, nel
terminale o dall'app (Impostazioni, Modelli e quote). XBot non usa chiavi delle API al posto degli
abbonamenti.

## Collegamenti

Gli assistenti usano server MCP locali (stdio) o remoti (HTTP, anche con OAuth). Il catalogo
nell'app propone i servizi più comuni; i token si salvano nella cassaforte e non compaiono nella
chat. Un assistente può aggiungere un collegamento dalla chat: il token si chiede in un riquadro
protetto.

## Voce

Dettato con faster-whisper sulla macchina del server (serve ffmpeg e un venv:
`./installa.sh --con-voce` lo crea). Risposta parlata con fish.audio, se metti la tua chiave in
`FISH_API_KEY`. Nell'app c'è anche la chat vocale a mani libere.

## Computer degli assistenti

Su un server Linux ogni assistente può avere un suo computer: uno schermo virtuale con Chrome e un
terminale, visibili dall'app (noVNC e ttyd) e chiusi con il servizio. Tutto ascolta su 127.0.0.1 e
passa dal server: nessuna porta in più verso l'esterno.

## Sicurezza

- Accesso con password e TOTP; i dispositivi fidati possono saltare il codice.
- Segreti generati sul posto dall'installatore, file d'ambiente 0600 (0640 sul server).
- Cassaforte cifrata con la chiave del file d'ambiente: se perdi la chiave, perdi i segreti salvati.
- Il segreto dei cookie cifra anche il TOTP: si cambia solo con `xbot-server ruota-segreto`.

## Licenza e codice di terzi

Licenza: vedi [LICENSE](LICENSE). Codice e materiali di terzi inclusi (noVNC, pako): vedi
[AVVISI-TERZI.md](AVVISI-TERZI.md).

## Contribuire

La guida tecnica per chi modifica il codice, persone o agenti, è [AGENTS.md](AGENTS.md):
struttura, regole del codice, prove. Prima di una richiesta di modifica: `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings` e `cargo test` puliti. Le segnalazioni vanno nelle
issue del repository; non incollare mai segreti, file d'ambiente o log con dati personali.
