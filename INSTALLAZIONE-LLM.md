# Installare XBot: istruzioni per un agente

Leggi questo file per intero prima di lanciare qualunque comando.

Sei un agente (Claude Code, Cursor, Codex, Gemini CLI o simile) e installi XBot per conto
dell'utente, sul suo computer o sul suo server. XBot è un server in Rust con un'interfaccia web: l'utente parla con i suoi assistenti, e gli assistenti lavorano con i client ufficiali dei suoi
abbonamenti (Claude Code, Cursor, Grok). L'installazione la fa `installa.sh` nella radice del
repository. Il tuo lavoro è scegliere le opzioni giuste con l'utente, lanciare lo script, verificare
e riferire.

## 1. Cosa chiedere all'utente

Fai queste domande una alla volta, prima di lanciare qualunque cosa. Per ognuna c'è un valore
predefinito: se l'utente non ha preferenze, usalo.

1. Il nome del bot orchestratore. È il nome dell'app, del primo assistente e dei servizi.
   Predefinito: `XBot`.
2. Il modo:
   - `locale`: il server gira su questo computer e ascolta solo su `127.0.0.1`. È il modo giusto
     per un Mac o un PC personale.
   - `vps`: il server gira come servizio di sistema su un Linux con systemd, dietro un proxy HTTPS.
     È il modo giusto per un server raggiungibile da internet (e dal telefono).
   - `worker`: questo computer non ha il server, si collega a un server XBot che esiste già.
   Se non sai quale proporre, lancia `uname -s` e `id -u`: macOS va in `locale`; Linux con
   systemd e root o sudo può andare in `vps`, ma chiedi.
3. Solo per `vps`: il dominio (per esempio `bot.example.org`) e se c'è già un Caddy o un altro
   proxy. Senza dominio il server resta raggiungibile solo dalla macchina stessa.
4. Solo per `worker`: l'indirizzo del server e il biglietto di collegamento (lo dà l'app in
   Impostazioni, Computer, Collega, oppure `./installa.sh --biglietto-nuovo` sul server). Il
   biglietto vale 15 minuti.
5. Il nome dell'utente nell'app (predefinito: il nome dell'utente del sistema).
6. Se vuole la voce (dettato con faster-whisper): serve scaricare pacchetti da PyPI, quindi chiedi.
7. Come vuole ricevere gli aggiornamenti: `auto` (ogni notte alle 04:00), `avvisa` (predefinito,
   solo l'avviso nell'app), `no`. Va in `--aggiornamento`.
8. Se manca Rust: chiedi il permesso di installarlo con rustup prima di aggiungere
   `--installa-rust`.

## 2. Cosa lanciare

Dalla radice del repository clonato. Con `--si` lo script non fa domande e non stampa segreti:
è il modo da usare per un agente.

```bash
# PC locale
./installa.sh --modo locale --nome "<nome>" --utente "<utente>" --si

# server (Linux, root o sudo)
sudo ./installa.sh --modo vps --nome "<nome>" --utente "<utente>" --dominio <dominio> --si

# un altro computer collegato a un server
./installa.sh --modo worker --url https://<dominio> --biglietto <biglietto> --si
```

Opzioni utili:

| Opzione | Effetto |
|---|---|
| `--porta <porta>` | porta d'ascolto su 127.0.0.1 (predefinita 8787); usala se la 8787 è occupata |
| `--ascolto <ip:porta>` | (vps) indirizzo d'ascolto, se il proxy sta in un contenitore |
| `--prefisso`, `--dati`, `--config` | cartelle diverse dalle predefinite (programmi, dati, file d'ambiente) |
| `--senza-servizio` | nessun LaunchAgent o systemd (anche su un Linux senza systemd, per esempio un contenitore): il server parte solo per la verifica e poi si ferma |
| `--senza-worker` | (locale) non collega il lavoratore di questo computer |
| `--senza-claude` | crea l'utente anche se `claude` non è nel PATH del servizio |
| `--aggiornamento auto\|avvisa\|no` | come arrivano gli aggiornamenti (vedi la sezione 7) |
| `--con-voce` | crea il venv di faster-whisper (scarica da PyPI) |
| `--installa-rust` | installa Rust con rustup se manca (scarica da sh.rustup.rs) |

Cosa fa lo script, in ordine: controlla i prerequisiti (git, compilatore C, Rust, python3, curl;
dice quali facoltativi mancano), compila `xbot-server` e `xbot-worker` in release, copia programmi,
interfaccia e voce nel prefisso, crea il file d'ambiente da `xbot.env.esempio` con i segreti
generati sul posto, crea l'utente con password e secondo fattore (TOTP), installa il servizio
(LaunchAgent su macOS, systemd utente su Linux locale, systemd di sistema sulla VPS), controlla
`/api/v1/salute`, crea il primo assistente con il nome del bot e, nel modo locale, collega il
lavoratore di questo computer. Sulla VPS con dominio scrive il blocco Caddy e lo stampa, senza
toccare un Caddy esistente.

Rilanciarlo è sicuro: aggiorna programmi, interfaccia e servizi; il database (con una copia in
`<dati>/backup/` prima di ogni aggiornamento), i segreti e l'utente restano quelli di prima.

La compilazione richiede alcuni minuti la prima volta. Lascia finire il comando.

Claude Code si registra quando lo script crea l'utente, cioè solo alla prima installazione. Se
`claude` non è nel PATH dell'utente del servizio, lo script si ferma subito dopo i prerequisiti,
prima della compilazione, e stampa i comandi per installarlo. Sulla VPS crea prima l'utente del
servizio (`xbot`): l'utente installa `claude` come `xbot` e fa l'accesso, poi tu rilanci lo stesso
comando. Con `--senza-claude` lo script prosegue senza. Chiedi all'utente se usa Claude Code prima
di lanciare.

## 3. Come verificare

1. L'uscita dello script finisce con «Fatto» e una riga `salute:` con `"ok":true`.
2. Lancia tu la stessa prova: `curl -s http://127.0.0.1:<porta>/api/v1/salute`. Deve rispondere
   `{"ok":true,...,"db":{"ok":true,"schema":"..."}}`.
3. Il servizio è attivo:
   - macOS: `launchctl print gui/$(id -u)/it.<sigla>.server | grep state`
   - Linux locale: `systemctl --user status <sigla>-server`
   - VPS: `systemctl status <sigla>` e `journalctl -u <sigla> -n 50`
   La sigla è il nome del bot in minuscolo, con i trattini al posto di spazi e simboli («Mio Bot»
   diventa `mio-bot`); lo script la stampa all'inizio come etichetta.
   Su Linux locale il servizio utente vuole una sessione vera (non `sudo` né ssh senza sessione) e,
   per restare acceso dopo il logout, `sudo loginctl enable-linger <utente>`: lo script lo dice, non
   lo fa.
4. Sulla VPS con dominio, dopo che l'utente ha aggiunto il blocco a Caddy:
   `curl -s https://<dominio>/api/v1/salute`.

Se qualcosa fallisce, leggi i log (`<dati>/log/` in locale, `journalctl` sulla VPS) e riferisci
l'errore con la riga esatta. Non correggere a mano il file d'ambiente dei segreti.

## 4. Il primo accesso

La password e l'indirizzo stanno in `<dati>/accesso-iniziale.txt` (0600); l'URL `otpauth://` del
secondo fattore sta in `<dati>/totp.txt` (0600). Non leggerli tu e non copiarli in chat. Di'
all'utente dove sono e come aprirli nel suo terminale:

```bash
cat ~/.local/share/xbot/accesso-iniziale.txt      # locale
sudo cat /var/lib/xbot/accesso-iniziale.txt        # vps
```

L'utente aggiunge l'URL del TOTP alla sua app di autenticazione (con `qrencode -t ANSIUTF8 <
totp.txt` vede il QR), poi apre `http://127.0.0.1:<porta>/app/` (locale) o `https://<dominio>/app/`
(vps). Dopo il primo accesso può cancellare `totp.txt`. Un codice TOTP vale una volta per finestra
di 30 secondi: se lo script ha appena fatto l'accesso, il primo codice può essere rifiutato e
basta aspettare il successivo.

## 5. Collegare i provider

I provider sono i client ufficiali degli abbonamenti dell'utente, lanciati come processi: Claude
Code (`claude`), Cursor (`agent`), Grok (`grok`). XBot non usa chiavi delle API al posto degli
abbonamenti.

- Lo script elenca i client che trova nel PATH del servizio. `claude` si registra da solo
  all'installazione; il lavoratore del computer dichiara anche Cursor, Grok e gli altri che trova.
- L'accesso ai client lo fa l'utente, mai tu. Non lanciare `claude login`, `agent login`,
  `grok login`, `codex login` o simili per conto suo, e non leggere i loro file di credenziali.
  Spiega i comandi e lascia che li lanci lui nel suo terminale:
  - locale: `claude` (al primo avvio chiede l'accesso), `agent login`, `grok login`;
  - vps: come utente del servizio. Installazione:
    `sudo -u xbot -H bash -c 'curl -fsSL https://claude.ai/install.sh | bash'`; accesso:
    `sudo -u xbot -H /var/lib/xbot/.local/bin/claude`. Lo stesso vale per `agent` e `grok`.
  In alternativa l'app ha la pagina Impostazioni, Modelli e quote, con l'accesso guidato.
- Se un client viene installato dopo XBot, rilancia lo script: il lavoratore lo vede al riavvio.

## 6. Collegare un altro computer

Sul server, con il server acceso: `./installa.sh --biglietto-nuovo` (stampa il comando con il
biglietto; a server spento dice «il server non risponde»), oppure
nell'app Impostazioni, Computer, Collega. Sull'altro computer, dopo `git clone`:

```bash
XBOT_BIGLIETTO=<biglietto> ./installa.sh --modo worker --url https://<dominio> --si
```

Con `XBOT_BIGLIETTO` il biglietto non compare nell'elenco dei processi (`--biglietto` funziona
lo stesso). Il biglietto vale 15 minuti ed è un segreto di breve durata: passalo dal server all'altro
computer senza scriverlo in file condivisi.

## 7. Aggiornare

Ogni versione pubblicata è un tag `vAAAA.MM.GG` (o `vAAAA.MM.GG.N`) del repository.

- Versione in opera e ultima pubblicata: `./installa.sh --versione`.
- Aggiornare adesso, dalla cartella del clone: `./installa.sh --aggiorna --si`. Lo script fa
  `git fetch --tags`, passa all'ultimo tag `v*` (con `--ramo main` segue il ramo), compila, tiene
  una copia dei programmi di prima (`<prefisso>/bin/*.precedente`, `web.precedente`,
  `voce.precedente`) e una del database, sostituisce, riavvia il servizio e aspetta `salute` fino a
  30 secondi. Se la compilazione fallisce non sostituisce niente; se il server nuovo non risponde
  rimette programmi, interfaccia e database di prima e riavvia. In tutti e due i casi esce con
  esito 1 e il clone torna alla versione di prima.
- Le opzioni dell'installazione si ricordano (in `.git/xbot-installa.conf` del clone): per
  aggiornare non vanno ripetute.
- Modo automatico: `./installa.sh --aggiornamento auto` installa un job launchd (macOS) o un timer
  systemd (Linux) che ogni giorno alle 04:00 lancia `installa.sh --aggiorna --si`.
  `--aggiornamento avvisa` (predefinito) lascia solo l'avviso nell'app; `--aggiornamento no` toglie
  job e timer. Chiedi all'utente quale vuole: con `auto` il server si riavvia da solo di notte.
- Registro: `<dati>/aggiornamenti.log`, una riga per tentativo (`data da -> a esito`); l'uscita del
  job automatico va in `<dati>/log/aggiorna.log` (macOS) o in `journalctl -u <sigla>-aggiorna`.
- Il bottone «Aggiorna ora» dell'app lancia `XBOT_AGGIORNA_COMANDO`, che installa.sh imposta
  perché l'aggiornamento giri fuori dal servizio del server (un riavvio non lo interrompe): un job
  launchd a parte su macOS, un'unità systemd utente su Linux, un'unità transitoria
  (`systemd-run`) sulla VPS. Sulla VPS l'utente `xbot` ha bisogno del permesso di systemd (polkit o
  una regola sudo): senza, l'aggiornamento si lancia da root con `./installa.sh --aggiorna` o con
  il timer. Se `--aggiorna` parte comunque dentro il servizio, si rilancia da solo con
  `systemd-run`.
- Con `--senza-servizio` lo script aggiorna i file; se un server avviato a mano è acceso, va
  riavviato a mano.
- Tornare indietro: `git checkout <tag di prima>` nel clone e `./installa.sh` (le scelte sono
  ricordate). Il database ha già le migrazioni della versione nuova: se la versione vecchia non
  parte, ferma il servizio e rimetti la copia del database in `<dati>/backup/` fatta prima
  dell'aggiornamento. Prima di tornare indietro su un'installazione dell'utente, chiedi.

## 8. Cosa non fare

- Non stampare, copiare o riassumere segreti: password, URL `otpauth://`, `XBOT_SEGRETO_COOKIE`,
  `XBOT_CASSAFORTE_CHIAVE`, chiavi VAPID, biglietti, credenziali dei client. Non aprire con `cat`
  il file d'ambiente, `accesso-iniziale.txt` o `totp.txt`.
- Non cambiare a mano `XBOT_SEGRETO_COOKIE`: cifra anche il seme TOTP e cambiarlo blocca
  l'accesso. Si ruota solo con `xbot-server ruota-segreto`.
- Non modificare servizi, LaunchAgent, unità systemd o Caddyfile che esistevano già. Lo script si
  ferma da solo se trova un file di servizio che non ha scritto lui: riferisci e chiedi.
- Non aprire porte nel firewall, non cambiare il DNS, non installare pacchetti di sistema senza
  dirlo all'utente e avere il suo sì. Lo script elenca i facoltativi che mancano: proponili, non
  installarli da solo.
- Non lanciare comandi di accesso dei client per conto dell'utente.
- Non cancellare la cartella dei dati: contiene il database, la cassaforte e i backup.
- Non mettere il file d'ambiente o la cartella dei dati in un repository o in una cartella
  sincronizzata con il cloud.

## 9. Come riferire all'utente

Alla fine scrivi un resoconto breve, senza segreti:

- modo, nome del bot, cartelle (programmi, dati, file d'ambiente) e servizio installato;
- l'uscita di `/api/v1/salute`;
- l'indirizzo da aprire e dove stanno `accesso-iniziale.txt` e `totp.txt`;
- i client trovati e quali accessi deve fare l'utente;
- cosa manca (facoltativi, Caddy, DNS, firewall) e cosa serve per completarlo;
- la versione installata (`./installa.sh --versione`) e il modo di aggiornamento scelto;
- ogni errore con la riga esatta del log.
