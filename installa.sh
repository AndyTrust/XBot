#!/usr/bin/env bash
# installa.sh: installa o aggiorna XBot su macOS o Linux.
#
#   ./installa.sh                         installazione guidata (fa le domande)
#   ./installa.sh --modo locale --nome XBot --si
#   ./installa.sh --modo vps --nome XBot --dominio bot.example.org --si
#   ./installa.sh --modo worker --url https://bot.example.org --biglietto <biglietto> --si
#   ./installa.sh --biglietto-nuovo       stampa un biglietto per collegare un altro computer
#   ./installa.sh --aggiorna              passa all'ultima versione (tag v*), con ripristino se fallisce
#   ./installa.sh --aggiornamento auto    aggiornamento automatico ogni giorno alle 04:00
#   ./installa.sh --versione              versione installata e ultima pubblicata
#
# Rilanciarlo aggiorna binari, interfaccia e servizi: database, segreti e utente restano.
# ./installa.sh --aiuto elenca tutte le opzioni.
set -euo pipefail

RADICE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SEGNO="Scritto da installa.sh di XBot"

# ------------------------------------------------------------------------------------------------
# Opzioni
# ------------------------------------------------------------------------------------------------
MODO=""; NOME=""; UTENTE=""; DOMINIO=""; ASCOLTO=""; PORTA=""
PREFISSO=""; DATI=""; CONFIG=""; URL_SERVER=""; BIGLIETTO=""
SI=0; SENZA_CLAUDE=0; SENZA_SERVIZIO=0; SENZA_WORKER=0; INSTALLA_RUST=0; CON_VOCE=0; BIGLIETTO_NUOVO=0
AGGIORNA=0; PASSO2=0; DA_REF=""; RAMO=""; MODO_AGG=""; SOLO_VERSIONE=0
ARGOMENTI=("$@")

aiuto() {
  cat <<'FINE'
Uso: ./installa.sh [opzioni]

  --modo locale|vps|worker   locale: PC con server su 127.0.0.1; vps: server di sistema dietro un
                             proxy HTTPS; worker: solo il lavoratore, collegato a un server
  --nome <nome>              nome del bot orchestratore (predefinito XBot)
  --utente <nome>            il tuo nome nell'app (predefinito: il nome dell'utente del sistema)
  --dominio <dominio>        (vps) dominio del proxy HTTPS; prepara il blocco Caddy
  --ascolto <ip:porta>       (vps) indirizzo d'ascolto del server (predefinito 127.0.0.1:8787)
  --porta <porta>            porta d'ascolto su 127.0.0.1 (predefinita 8787)
  --prefisso <cartella>      dove vanno bin/, web/, voce/ (locale ~/.local/opt/xbot, vps /opt/xbot)
  --dati <cartella>          cartella dei dati (locale ~/.local/share/xbot, vps /var/lib/xbot)
  --config <file>            file d'ambiente (locale ~/.config/xbot/xbot.env, vps /etc/xbot/xbot.env)
  --url <url>                (worker) indirizzo del server
  --biglietto <biglietto>    (worker) biglietto di collegamento, vale 15 minuti; meglio con la
                             variabile XBOT_BIGLIETTO, che non compare in ps
  --biglietto-nuovo          stampa un biglietto nuovo dal server installato qui e termina
  --senza-servizio           niente LaunchAgent o systemd: avvio temporaneo solo per la verifica
  --senza-worker             (locale) non collega il lavoratore di questo computer
  --senza-claude             impianta anche se `claude` non è nel PATH del servizio (si registra
                             solo Claude Code, e solo alla prima installazione)
  --con-voce                 crea il venv di faster-whisper (scarica pacchetti da PyPI)
  --installa-rust            con --si: installa Rust con rustup se manca (scarica da sh.rustup.rs)
  --aggiorna                 git fetch, ultimo tag v* (o --ramo), compila, sostituisce, riavvia e
                             controlla salute; se qualcosa fallisce rimette la versione di prima
  --ramo <ramo>              con --aggiorna: segue il ramo (es. main) invece dei tag
  --aggiornamento auto|avvisa|no
                             auto: aggiornamento ogni giorno alle 04:00; avvisa (predefinito): solo
                             l'avviso nell'app; no: niente. Su un'installazione esistente cambia
                             solo questo e termina
  --versione                 stampa la versione installata e l'ultima pubblicata
  --si                       nessuna domanda: valgono le opzioni e i predefiniti
  --aiuto                    questo testo
FINE
}

while [ $# -gt 0 ]; do
  case "$1" in
    --modo) MODO="${2:-}"; shift 2 ;;
    --nome) NOME="${2:-}"; shift 2 ;;
    --utente) UTENTE="${2:-}"; shift 2 ;;
    --dominio) DOMINIO="${2:-}"; shift 2 ;;
    --ascolto) ASCOLTO="${2:-}"; shift 2 ;;
    --porta) PORTA="${2:-}"; shift 2 ;;
    --prefisso) PREFISSO="${2:-}"; shift 2 ;;
    --dati) DATI="${2:-}"; shift 2 ;;
    --config) CONFIG="${2:-}"; shift 2 ;;
    --url) URL_SERVER="${2:-}"; shift 2 ;;
    --biglietto) BIGLIETTO="${2:-}"; shift 2 ;;
    --biglietto-nuovo) BIGLIETTO_NUOVO=1; shift ;;
    --senza-servizio) SENZA_SERVIZIO=1; shift ;;
    --senza-worker) SENZA_WORKER=1; shift ;;
    --con-voce) CON_VOCE=1; shift ;;
    --senza-claude) SENZA_CLAUDE=1; shift ;;
    --installa-rust) INSTALLA_RUST=1; shift ;;
    --aggiorna) AGGIORNA=1; shift ;;
    --aggiorna-passo2) PASSO2=1; AGGIORNA=1; shift ;;
    --da) DA_REF="${2:-}"; shift 2 ;;
    --ramo) RAMO="${2:-}"; shift 2 ;;
    --aggiornamento) MODO_AGG="${2:-}"; shift 2 ;;
    --versione) SOLO_VERSIONE=1; shift ;;
    --si) SI=1; shift ;;
    -h|--aiuto|--help) aiuto; exit 0 ;;
    *) echo "Opzione sconosciuta: $1 (vedi --aiuto)" >&2; exit 2 ;;
  esac
done
case "$MODO_AGG" in ""|auto|avvisa|no) ;; *) echo "--aggiornamento vuole auto, avvisa o no" >&2; exit 2 ;; esac
[ "$PASSO2" -eq 0 ] || SI=1

# Le scelte dell'installazione stanno nel clone, dentro .git (mai tracciate): --aggiorna, il timer
# e --versione le rileggono senza opzioni.
if GIT_DIR_CLONE="$(git -C "$RADICE" rev-parse --absolute-git-dir 2>/dev/null)"; then
  FILE_SCELTE="$GIT_DIR_CLONE/xbot-installa.conf"
else
  FILE_SCELTE="$RADICE/.xbot-installa.conf"
fi
CONF_ESISTE=0; [ -f "$FILE_SCELTE" ] && CONF_ESISTE=1
SOLO_MODO_AGG=0
if [ -n "$MODO_AGG" ] && [ "$CONF_ESISTE" -eq 1 ] && [ "$AGGIORNA" -eq 0 ] && [ "$BIGLIETTO_NUOVO" -eq 0 ] && [ "$SOLO_VERSIONE" -eq 0 ]; then
  SOLO_MODO_AGG=1
fi
if [ "$CONF_ESISTE" -eq 1 ] && { [ "$AGGIORNA" -eq 1 ] || [ "$SOLO_VERSIONE" -eq 1 ] || [ "$BIGLIETTO_NUOVO" -eq 1 ] || [ "$SOLO_MODO_AGG" -eq 1 ]; }; then
  # shellcheck disable=SC1090
  . "$FILE_SCELTE"
  [ -n "$MODO" ] || MODO="${S_MODO:-}"
  [ -n "$NOME" ] || NOME="${S_NOME:-}"
  [ -n "$UTENTE" ] || UTENTE="${S_UTENTE:-}"
  [ -n "$DOMINIO" ] || DOMINIO="${S_DOMINIO:-}"
  [ -n "$ASCOLTO" ] || ASCOLTO="${S_ASCOLTO:-}"
  [ -n "$PREFISSO" ] || PREFISSO="${S_PREFISSO:-}"
  [ -n "$DATI" ] || DATI="${S_DATI:-}"
  [ -n "$CONFIG" ] || CONFIG="${S_CONFIG:-}"
  [ -n "$URL_SERVER" ] || URL_SERVER="${S_URL_SERVER:-}"
  [ -n "$RAMO" ] || RAMO="${S_RAMO:-}"
  [ -n "$MODO_AGG" ] || MODO_AGG="${S_MODO_AGG:-}"
  [ "$SENZA_SERVIZIO" -eq 1 ] || SENZA_SERVIZIO="${S_SENZA_SERVIZIO:-0}"
  [ "$SENZA_WORKER" -eq 1 ] || SENZA_WORKER="${S_SENZA_WORKER:-0}"
  [ -n "${CARGO_TARGET_DIR:-}" ] || { [ -z "${S_CARGO_TARGET_DIR:-}" ] || export CARGO_TARGET_DIR="$S_CARGO_TARGET_DIR"; }
  SI=1
fi

# ------------------------------------------------------------------------------------------------
# Utilità
# ------------------------------------------------------------------------------------------------
di() { printf '%s\n' "$*"; }
passo() { printf '\n== %s\n' "$*"; }
avviso() { printf 'Attenzione: %s\n' "$*" >&2; }
errore() { printf 'Errore: %s\n' "$*" >&2; exit 1; }

interattivo() { [ "$SI" -eq 0 ] && [ -t 0 ]; }

# chiedi <variabile> <domanda> <predefinito>
chiedi() {
  local var="$1" domanda="$2" pred="$3" r=""
  if interattivo; then
    read -r -p "$domanda [$pred]: " r || true
  fi
  [ -n "$r" ] || r="$pred"
  printf -v "$var" '%s' "$r"
}

# conferma <domanda>: vero con «s». Senza terminale o con --si: falso (niente azioni non chieste).
conferma() {
  local r=""
  interattivo || return 1
  read -r -p "$1 [s/N]: " r || true
  case "$r" in s|S|si|sì|y|Y) return 0 ;; *) return 1 ;; esac
}

c_e() { command -v "$1" >/dev/null 2>&1; }

minuscolo() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]'; }

# Nome per etichette e file: minuscole, cifre e trattini.
abbrevia() {
  local s
  s="$(minuscolo "$1" | tr -c 'a-z0-9' '-' | sed -E 's/-+/-/g; s/^-//; s/-$//')"
  [ -n "$s" ] || s="xbot"
  printf '%s' "$s"
}

segreto_base64() {
  if c_e openssl; then openssl rand -base64 32
  else head -c 32 /dev/urandom | base64 | tr -d '\n'; fi
}

password_nuova() {
  if c_e openssl; then openssl rand -base64 24 | tr -d '/+=\n' | cut -c1-20
  else head -c 24 /dev/urandom | base64 | tr -d '/+=\n' | cut -c1-20; fi
}

ora() { date '+%Y-%m-%d %H:%M'; }

# Comandi da amministratore (solo nel modo vps).
SUDO=""
come_root() { if [ -n "$SUDO" ]; then "$SUDO" "$@"; else "$@"; fi; }

# Comandi come utente del servizio (vps: l'utente di sistema; altrove chi lancia lo script),
# sempre con il PATH del servizio (sudo lo azzererebbe).
UTENTE_SERVIZIO=""
PATH_SERVIZIO="$HOME/.local/bin:$PATH"
# Con SOSTITUISCI=1 il comando prende il posto della shell (exec): serve per l'avvio in
# background, così il PID salvato è quello del server e kill lo ferma davvero.
SOSTITUISCI=0
come_servizio() {
  local x=""; [ "$SOSTITUISCI" -eq 0 ] || x="exec"
  if [ "$MODO" != "vps" ] || [ "$(id -un)" = "$UTENTE_SERVIZIO" ]; then $x env PATH="$PATH_SERVIZIO" "$@"; return; fi
  if [ "$(id -u)" -eq 0 ] && c_e runuser; then $x runuser -u "$UTENTE_SERVIZIO" -- env PATH="$PATH_SERVIZIO" "$@"
  else $x sudo -u "$UTENTE_SERVIZIO" --preserve-env=XBOT_PASSWORD env PATH="$PATH_SERVIZIO" "$@"; fi
}

CARTELLA_TMP="$(mktemp -d "${TMPDIR:-/tmp}/xbot-installa.XXXXXX")"
chmod 700 "$CARTELLA_TMP"
temporaneo() { mktemp "$CARTELLA_TMP/f.XXXXXX"; }
PID_PROVA=""
pulisci() {
  rm -rf "$CARTELLA_TMP"
  if [ -n "$PID_PROVA" ]; then kill "$PID_PROVA" 2>/dev/null || true; fi
}
trap pulisci EXIT

# --- Python di servizio (letti in variabili, passati con -c) ---------------------------------

# Imposta NOME="valore" nel file d'ambiente. Valore da stdin. Argomenti: file nome solo_se_manca.
read -r -d '' PY_IMPOSTA <<'PY' || true
import pathlib, sys
p, k, solo = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3] == "1"
v = sys.stdin.read().strip()
righe = p.read_text().splitlines() if p.exists() else []
nuova = '%s="%s"' % (k, v)
for i, r in enumerate(righe):
    if r.startswith(k + "="):
        if not solo:
            righe[i] = nuova
        break
else:
    for i, r in enumerate(righe):
        if r.startswith("# " + k + "="):
            righe[i] = nuova
            break
    else:
        righe.append(nuova)
p.write_text("\n".join(righe) + "\n")
PY

# Esce con 0 se il file d'ambiente ha la variabile attiva e non vuota.
read -r -d '' PY_HA <<'PY' || true
import pathlib, sys
p, k = pathlib.Path(sys.argv[1]), sys.argv[2]
for r in (p.read_text().splitlines() if p.exists() else []):
    if r.startswith(k + "=") and r.split("=", 1)[1].strip().strip('"'):
        sys.exit(0)
sys.exit(1)
PY

# Riempie un modello: @CHIAVE@ diventa il valore di R_CHIAVE. Argomenti: modello xml(0|1).
read -r -d '' PY_RIEMPI <<'PY' || true
import os, re, sys
t = open(sys.argv[1]).read()
xml = sys.argv[2] == "1"
def valore(m):
    v = os.environ.get("R_" + m.group(1))
    if v is None:
        return m.group(0)
    if xml:
        v = v.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
    return v
sys.stdout.write(re.sub(r"@([A-Z]+)@", valore, t))
PY

# Il nome del bot nella copia installata di web/ (come web/prove/rinomina.py). Argomenti: web nome.
read -r -d '' PY_RINOMINA <<'PY' || true
import json, pathlib, re, sys
web, nome = pathlib.Path(sys.argv[1]), sys.argv[2]
def cambia(f, fn):
    p = web / f
    if p.exists():
        p.write_text(fn(p.read_text()))
cambia("nome.js", lambda t: re.sub(r'NOME_APP = "[^"]*"', "NOME_APP = %s" % json.dumps(nome), t))
cambia("sw.js", lambda t: re.sub(r'const NOME_APP = "[^"]*"', "const NOME_APP = %s" % json.dumps(nome), t))
m = web / "manifest.webmanifest"
if m.exists():
    d = json.loads(m.read_text())
    d["name"] = d["short_name"] = nome
    m.write_text(json.dumps(d, ensure_ascii=False, indent=2) + "\n")
def html(t):
    t = re.sub(r"<title>[^<]*</title>", "<title>%s</title>" % nome, t)
    return re.sub(r'(<h1 id="nome-app">)[^<]*(</h1>)', lambda g: g.group(1) + nome + g.group(2), t)
cambia("index.html", html)
PY

# Accede con password e TOTP dai file e fa un'azione. Argomenti: url accesso totp azione [nome].
# azione: assistente (crea il primo assistente se manca) | biglietto (stampa un biglietto).
read -r -d '' PY_CHIAMA <<'PY' || true
import base64, hashlib, hmac, json, struct, sys, time, urllib.error, urllib.parse, urllib.request
url, f_accesso, f_totp, azione = sys.argv[1:5]
nome = sys.argv[5] if len(sys.argv) > 5 else ""
password = None
for r in open(f_accesso):
    if r.startswith("password:"):
        password = r.split(":", 1)[1].strip()
if not password:
    sys.exit("password non trovata in " + f_accesso)
try:
    q = urllib.parse.parse_qs(urllib.parse.urlparse(open(f_totp).read().strip()).query)
except FileNotFoundError:
    q = None
def codice():
    s = q["secret"][0]
    k = base64.b32decode(s + "=" * (-len(s) % 8), casefold=True)
    passo = int(q.get("period", ["30"])[0])
    cifre = int(q.get("digits", ["6"])[0])
    alg = {"SHA1": hashlib.sha1, "SHA256": hashlib.sha256, "SHA512": hashlib.sha512}[q.get("algorithm", ["SHA1"])[0].upper()]
    h = hmac.new(k, struct.pack(">Q", int(time.time()) // passo), alg).digest()
    o = h[-1] & 15
    return str((struct.unpack(">I", h[o:o + 4])[0] & 0x7FFFFFFF) % 10 ** cifre).zfill(cifre)
biscotti = {}
def chiama(metodo, percorso, corpo=None):
    try:
        return chiama_davvero(metodo, percorso, corpo)
    except (urllib.error.URLError, OSError) as e:
        sys.exit("il server non risponde su %s (%s): avvialo prima" % (url, getattr(e, "reason", e)))
def chiama_davvero(metodo, percorso, corpo=None):
    dati = None if corpo is None else json.dumps(corpo).encode()
    req = urllib.request.Request(url + "/api/v1" + percorso, data=dati, method=metodo)
    req.add_header("content-type", "application/json")
    if biscotti:
        req.add_header("cookie", "; ".join("%s=%s" % kv for kv in biscotti.items()))
    try:
        r = urllib.request.urlopen(req, timeout=30)
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode(errors="replace"), e.headers
    return r.status, r.read().decode(errors="replace"), r.headers
def entra():
    for prova in range(2):
        corpo = {"password": password, "dispositivo": "installa.sh"}
        if q:
            corpo["codice_totp"] = codice()
        stato, testo, intest = chiama("POST", "/accesso", corpo)
        if stato == 200:
            for c in intest.get_all("set-cookie") or []:
                k, _, v = c.split(";", 1)[0].partition("=")
                biscotti[k.strip()] = v.strip()
            return
        if prova == 0 and q:
            time.sleep(31)  # un codice vale una volta per finestra di 30 secondi
            continue
        sys.exit("accesso rifiutato (%d): %s" % (stato, testo[:200]))
entra()
if azione == "assistente":
    stato, testo, _ = chiama("GET", "/assistenti")
    if stato != 200:
        sys.exit("elenco assistenti: %d" % stato)
    if any(a.get("nome") == nome for a in json.loads(testo)):
        print("già presente")
    else:
        stato, testo, _ = chiama("POST", "/assistenti", {"nome": nome, "descrizione": "Orchestratore"})
        if stato not in (200, 201):
            sys.exit("creazione assistente: %d %s" % (stato, testo[:200]))
        print("creato")
elif azione == "biglietto":
    stato, testo, _ = chiama("POST", "/macchine/biglietto", {})
    if stato != 200:
        sys.exit("biglietto: %d %s" % (stato, testo[:200]))
    print(json.loads(testo)["biglietto"])
PY

imposta() { printf '%s' "$2" | come_root python3 -c "$PY_IMPOSTA" "$CONFIG" "$1" 0; }
imposta_se_manca() { printf '%s' "$2" | come_root python3 -c "$PY_IMPOSTA" "$CONFIG" "$1" 1; }
ha_var() { come_root python3 -c "$PY_HA" "$CONFIG" "$1"; }

# riempi <modello> <destinazione> <xml 0|1> <permessi>
riempi() {
  local t; t="$(temporaneo)"
  R_NOME="$NOME" R_ETICHETTA="$ETICHETTA" R_PREFISSO="$PREFISSO" R_DATI="$DATI" R_CONFIG="$CONFIG" \
  R_UTENTE="$UTENTE_SERVIZIO" R_PATH="$PATH_SERVIZIO" R_HOME="$HOME" R_WORKER="$WORKER_DIR" \
  R_DOMINIO="$DOMINIO" R_ASCOLTO="$ASCOLTO" R_SIGLA="$SIGLA" R_SORGENTE="$RADICE" \
  R_PATHAGG="${PATH_AGGIORNA:-$PATH}" R_CALENDARIO="${CALENDARIO:-}" \
    python3 -c "$PY_RIEMPI" "$1" "$3" >"$t"
  come_root mkdir -p "$(dirname "$2")"
  come_root install -m "$4" "$t" "$2"
}

# Un file di servizio esistente si sostituisce solo se l'ha scritto questo script.
nostro_o_assente() { [ ! -e "$1" ] || come_root grep -q "$SEGNO" "$1"; }

salute() { curl -fsS --max-time 3 "http://$ASCOLTO/api/v1/salute" 2>/dev/null; }

aspetta_salute() {
  local i
  for i in $(seq 1 30); do
    if salute >/dev/null; then return 0; fi
    sleep 1
  done
  return 1
}

# ------------------------------------------------------------------------------------------------
# Versioni e aggiornamenti
# ------------------------------------------------------------------------------------------------
# Ordina nomi di versione (v2026.10.25, v2026.10.25.2) e stampa il più recente.
ultima_versione() {
  python3 -c 'import re,sys
v=[r.strip() for r in sys.stdin if r.strip()]
print(max(v, key=lambda s: [int(n) for n in re.findall(r"\d+", s)]) if v else "")'
}

versione_sorgente() {
  git -C "$RADICE" describe --tags --exact-match 2>/dev/null \
    || git -C "$RADICE" rev-parse --short HEAD 2>/dev/null || echo sconosciuta
}

versione_di() { git -C "$RADICE" describe --tags --exact-match "$1" 2>/dev/null || git -C "$RADICE" rev-parse --short "$1" 2>/dev/null || echo "$1"; }

salva_scelte() {
  {
    printf '# %s: scelte dell'"'"'installazione (%s). Niente segreti.\n' "$SEGNO" "$(ora)"
    printf 'S_MODO=%q\nS_NOME=%q\nS_UTENTE=%q\nS_DOMINIO=%q\nS_ASCOLTO=%q\n' "$MODO" "$NOME" "$UTENTE" "$DOMINIO" "$ASCOLTO"
    printf 'S_PREFISSO=%q\nS_PREFISSO_SCELTO=%q\nS_DATI=%q\nS_CONFIG=%q\nS_URL_SERVER=%q\n' "$PREFISSO" "$PREFISSO_SCELTO" "$DATI" "$CONFIG" "$URL_SERVER"
    printf 'S_RAMO=%q\nS_MODO_AGG=%q\nS_SENZA_SERVIZIO=%q\nS_SENZA_WORKER=%q\nS_CARGO_TARGET_DIR=%q\n' "$RAMO" "$MODO_AGG" "$SENZA_SERVIZIO" "$SENZA_WORKER" "${CARGO_TARGET_DIR:-}"
  } >"$FILE_SCELTE"
}

scrivi_versione() {
  local t; t="$(temporaneo)"
  printf '%s %s\n' "$(versione_sorgente)" "$(ora)" >"$t"
  come_root install -m 644 "$t" "$DATI/versione.txt"
}

# registra <esito> <dettaglio>: una riga in <dati>/aggiornamenti.log.
registra_aggiornamento() {
  local riga
  riga="$(ora) $(versione_di "${DA_REF:-HEAD}") -> ${VERSIONE_NUOVA:-$(versione_sorgente)} $1${2:+ ($2)}"
  come_root mkdir -p "$DATI"
  printf '%s\n' "$riga" | come_root tee -a "$DATI/aggiornamenti.log" >/dev/null
}

# Passo 1 di --aggiorna (con lo script installato): porta il clone alla versione nuova e rilancia
# lo script nuovo, che compila e installa (passo 2).
# Lanciato dal server («Aggiorna ora»), lo script starebbe nel cgroup del servizio: il riavvio lo
# ucciderebbe a metà. Se succede, si rilancia in un'unità transitoria di systemd ed esce.
staccati_dal_servizio() {
  local cg="/proc/self/cgroup" utente=""
  [ -r "$cg" ] && [ -z "${XBOT_AGGIORNA_STACCATO:-}" ] && c_e systemd-run || return 0
  grep -qE "/(xbot|$SIGLA|$SIGLA-server)\.service" "$cg" || return 0
  [ "$(id -u)" -eq 0 ] || utente="--user"
  if systemd-run $utente --unit="$SIGLA-aggiorna-ora" --collect --quiet --setenv=XBOT_AGGIORNA_STACCATO=1 \
      /bin/bash "$RADICE/installa.sh" ${ARGOMENTI[@]+"${ARGOMENTI[@]}"}; then
    di "Aggiornamento rilanciato fuori dal servizio: unità $SIGLA-aggiorna-ora (journalctl ${utente:+--user }-u $SIGLA-aggiorna-ora)."
    exit 0
  fi
  avviso "systemd-run non riuscito: proseguo, ma il riavvio del servizio può interrompere questo script"
}

aggiorna_sorgente() {
  local prima dest
  staccati_dal_servizio
  passo "Aggiornamento del sorgente in $RADICE"
  git -C "$RADICE" rev-parse --git-dir >/dev/null 2>&1 || errore "$RADICE non è un clone git: non posso aggiornare"
  if [ -n "$(git -C "$RADICE" status --porcelain --untracked-files=no)" ]; then
    errore "nel clone ci sono modifiche locali ai file tracciati: salvale o annullale (git status) e rilancia"
  fi
  prima="$(git -C "$RADICE" rev-parse HEAD)"
  git -C "$RADICE" fetch --quiet --tags origin || errore "git fetch non riuscito: controlla la rete e il remoto origin"
  if [ -n "$RAMO" ]; then
    git -C "$RADICE" fetch --quiet origin "$RAMO" || errore "il ramo $RAMO non c'è su origin"
    dest="origin/$RAMO"
  else
    dest="$(git -C "$RADICE" tag -l 'v*' | ultima_versione)"
    [ -n "$dest" ] || errore "nessun tag v* nel repository: per seguire un ramo usa --ramo main"
  fi
  if [ "$(git -C "$RADICE" rev-parse "$dest^{commit}")" = "$prima" ]; then
    di "Già all'ultima versione: $(versione_sorgente)."
    if [ -n "$DATI" ] && come_root test -d "$DATI"; then DA_REF="$prima"; registra_aggiornamento "già aggiornato"; fi
    exit 0
  fi
  di "Da $(versione_di "$prima") a $(versione_di "$dest")."
  git -C "$RADICE" checkout --quiet --detach "$dest"
  exec bash "$RADICE/installa.sh" ${ARGOMENTI[@]+"${ARGOMENTI[@]}"} --aggiorna-passo2 --da "$prima"
}

# Fallimento nel passo 2: il clone torna alla versione di prima, una riga nel registro, esito 1.
torna_sorgente() {
  [ -n "$DA_REF" ] && git -C "$RADICE" checkout --quiet --detach "$DA_REF" 2>/dev/null || true
}

copia_precedente() {
  local b c
  for b in xbot-server xbot-worker; do
    if come_root test -f "$PREFISSO/bin/$b"; then come_root cp -p "$PREFISSO/bin/$b" "$PREFISSO/bin/$b.precedente"; fi
  done
  for c in web voce; do
    if come_root test -d "$PREFISSO/$c"; then
      if c_e rsync; then come_root rsync -a --delete "$PREFISSO/$c/" "$PREFISSO/$c.precedente/"
      else come_root rm -rf "$PREFISSO/$c.precedente"; come_root cp -R "$PREFISSO/$c" "$PREFISSO/$c.precedente"; fi
    fi
  done
}

rimetti_precedente() {
  local b c
  for b in xbot-server xbot-worker; do
    if come_root test -f "$PREFISSO/bin/$b.precedente"; then come_root install -m 755 "$PREFISSO/bin/$b.precedente" "$PREFISSO/bin/$b"; fi
  done
  for c in web voce; do
    if come_root test -d "$PREFISSO/$c.precedente"; then
      if c_e rsync; then come_root rsync -a --delete "$PREFISSO/$c.precedente/" "$PREFISSO/$c/"
      else come_root rm -rf "$PREFISSO/$c"; come_root cp -R "$PREFISSO/$c.precedente" "$PREFISSO/$c"; fi
    fi
  done
}

ferma_server() {
  case "$MODO:$SISTEMA" in
    locale:Darwin) launchctl bootout "gui/$(id -u)/$ETICHETTA.server" 2>/dev/null || true ;;
    locale:Linux) systemctl --user stop "$SIGLA-server.service" 2>/dev/null || true ;;
    vps:Linux) come_root systemctl stop "$SIGLA.service" 2>/dev/null || true ;;
  esac
}

# Il server nuovo non risponde: binari, interfaccia e database di prima, riavvio, esito 1.
ripristina_aggiornamento() {
  local motivo="$1" stato="no"
  avviso "$motivo: rimetto la versione di prima"
  if [ -n "$PID_PROVA" ]; then kill "$PID_PROVA" 2>/dev/null || true; wait "$PID_PROVA" 2>/dev/null || true; PID_PROVA=""
  else ferma_server; fi
  rimetti_precedente
  if [ -n "${COPIA:-}" ] && come_root test -f "$COPIA"; then
    come_root cp "$COPIA" "$DATI/xbot.sqlite"
    come_root rm -f "$DATI/xbot.sqlite-wal" "$DATI/xbot.sqlite-shm"
    [ "$MODO" != "vps" ] || come_root chown "$UTENTE_SERVIZIO:$UTENTE_SERVIZIO" "$DATI/xbot.sqlite"
    di "Database rimesso dalla copia $COPIA."
  fi
  torna_sorgente
  if [ "$SENZA_SERVIZIO" -eq 0 ]; then
    installa_servizio_server >/dev/null
    if aspetta_salute; then stato="ok"; fi
  else
    # Senza servizio: prova della versione di prima con un avvio temporaneo, poi si ferma.
    SOSTITUISCI=1 come_servizio "$PREFISSO/bin/xbot-avvia" >>"$DATI/log/avvio-prova.log" 2>&1 &
    PID_PROVA=$!
    if aspetta_salute; then stato="ok"; fi
    kill "$PID_PROVA" 2>/dev/null || true; wait "$PID_PROVA" 2>/dev/null || true; PID_PROVA=""
  fi
  registra_aggiornamento "ripristinato" "$motivo; salute dopo il ripristino: $stato"
  errore "aggiornamento non riuscito ($motivo). In opera resta $(versione_sorgente); salute: $stato. Registro: $DATI/aggiornamenti.log"
}

# systemd utente: serve il bus della sessione; senza linger il servizio si ferma al logout.
# Senza systemd o senza bus: con --senza-servizio solo un avviso (esce 1, il servizio si salta),
# altrimenti lo script si ferma con le due strade.
controlla_systemd_utente() {
  local motivo=""
  if ! c_e systemctl; then
    motivo="nessun systemd su questa macchina: avvio manuale con $PREFISSO/bin/xbot-avvia, oppure rilancia con --senza-servizio"
  elif ! systemctl --user show-environment >/dev/null 2>&1; then
    motivo="systemctl --user non raggiunge il bus della sessione (da sudo o da ssh senza sessione): entra come $(id -un) in una sessione vera, con sudo loginctl enable-linger $(id -un) per tenerlo acceso dopo il logout, oppure rilancia con --senza-servizio"
  fi
  if [ -n "$motivo" ]; then
    if [ "$SENZA_SERVIZIO" -eq 1 ]; then avviso "$motivo"; return 1; fi
    errore "$motivo"
  fi
  if c_e loginctl && [ "$(loginctl show-user "$(id -un)" -p Linger --value 2>/dev/null)" != "yes" ]; then
    avviso "senza linger il servizio si ferma quando esci dalla sessione: per tenerlo acceso, sudo loginctl enable-linger $(id -un)"
  fi
}

# Modo di aggiornamento: XBOT_AGGIORNAMENTO nell'env e job o timer giornaliero alle 04:00.
#   auto: job/timer attivo · avvisa: solo il lavoro a richiesta · no: niente.
applica_modo_aggiornamento() {
  local m="${MODO_AGG:-avvisa}" plist unita uid
  MODO_AGG="$m"
  if [ "$MODO" != "worker" ] && come_root test -f "$CONFIG"; then imposta XBOT_AGGIORNAMENTO "$m"; fi
  PATH_AGGIORNA="$(dirname "$(command -v cargo 2>/dev/null || echo /usr/bin/cargo)"):$(dirname "$(command -v git)"):$PATH"
  if [ "$SENZA_SERVIZIO" -eq 1 ]; then
    [ "$m" != "auto" ] || avviso "con --senza-servizio non installo il timer: l'aggiornamento automatico va lanciato a mano (./installa.sh --aggiorna)"
    return 0
  fi
  case "$MODO:$SISTEMA" in
    locale:Darwin|worker:Darwin)
      plist="$HOME/Library/LaunchAgents/$ETICHETTA.aggiorna.plist"; uid="$(id -u)"
      nostro_o_assente "$plist" || { avviso "$plist non è di questo script: non lo tocco"; return 0; }
      launchctl bootout "gui/$uid/$ETICHETTA.aggiorna" 2>/dev/null || true
      if [ "$m" = "no" ]; then rm -f "$plist"; di "Aggiornamento: no (nessun job)."; return 0; fi
      CALENDARIO=""
      [ "$m" != "auto" ] || CALENDARIO='  <key>StartCalendarInterval</key><dict><key>Hour</key><integer>4</integer><key>Minute</key><integer>0</integer></dict>'
      riempi "$RADICE/ops/launchd/aggiorna.plist.modello" "$plist" 1 644
      launchctl bootstrap "gui/$uid" "$plist"
      [ "$MODO" = "worker" ] || imposta XBOT_AGGIORNA_COMANDO "launchctl kickstart gui/$uid/$ETICHETTA.aggiorna"
      di "Aggiornamento: $m (job $plist)." ;;
    locale:Linux|worker:Linux)
      controlla_systemd_utente || return 0
      unita="$HOME/.config/systemd/user/$SIGLA-aggiorna"
      nostro_o_assente "$unita.service" || { avviso "$unita.service non è di questo script: non lo tocco"; return 0; }
      systemctl --user disable --now "$SIGLA-aggiorna.timer" >/dev/null 2>&1 || true
      if [ "$m" = "no" ]; then rm -f "$unita.service" "$unita.timer"; systemctl --user daemon-reload; di "Aggiornamento: no."; return 0; fi
      riempi "$RADICE/ops/systemd/utente/aggiorna.service" "$unita.service" 0 644
      riempi "$RADICE/ops/systemd/utente/aggiorna.timer" "$unita.timer" 0 644
      systemctl --user daemon-reload
      [ "$m" != "auto" ] || systemctl --user enable --now "$SIGLA-aggiorna.timer" >/dev/null
      [ "$MODO" = "worker" ] || imposta XBOT_AGGIORNA_COMANDO "systemctl --user start --no-block $SIGLA-aggiorna.service"
      di "Aggiornamento: $m (unità $unita.service)." ;;
    vps:Linux)
      unita="/etc/systemd/system/$SIGLA-aggiorna"
      nostro_o_assente "$unita.service" || { avviso "$unita.service non è di questo script: non lo tocco"; return 0; }
      come_root systemctl disable --now "$SIGLA-aggiorna.timer" >/dev/null 2>&1 || true
      if [ "$m" = "no" ]; then come_root rm -f "$unita.service" "$unita.timer"; come_root systemctl daemon-reload; di "Aggiornamento: no."; return 0; fi
      riempi "$RADICE/ops/systemd/xbot-aggiorna.service" "$unita.service" 0 644
      riempi "$RADICE/ops/systemd/xbot-aggiorna.timer" "$unita.timer" 0 644
      come_root systemctl daemon-reload
      [ "$m" != "auto" ] || come_root systemctl enable --now "$SIGLA-aggiorna.timer" >/dev/null
      # Un'unità transitoria a parte: il riavvio di $SIGLA.service non la tocca. Da $UTENTE_SERVIZIO
      # serve il permesso di systemd (polkit o sudo): senza, «Aggiorna ora» dell'app non parte.
      imposta XBOT_AGGIORNA_COMANDO "systemd-run --unit=$SIGLA-aggiorna-ora --collect /bin/bash $RADICE/installa.sh --aggiorna --si"
      di "Aggiornamento: $m (unità $unita.service)." ;;
  esac
}

# ------------------------------------------------------------------------------------------------
# 1. Saluto e nome del bot
# ------------------------------------------------------------------------------------------------
SISTEMA="$(uname -s)"
if [ "$BIGLIETTO_NUOVO" -eq 0 ] && [ "$SOLO_VERSIONE" -eq 0 ] && [ "$SOLO_MODO_AGG" -eq 0 ]; then
  di "Installazione di XBot ($(ora)), dalla cartella $RADICE"
  di "Il bot orchestratore è l'assistente che apre l'app e coordina gli altri."
  [ -n "$NOME" ] || chiedi NOME "Nome del bot orchestratore" "XBot"
fi
[ -n "$NOME" ] || NOME="XBot"
SIGLA="$(abbrevia "$NOME")"
ETICHETTA="it.$SIGLA"

# ------------------------------------------------------------------------------------------------
# 2. Modo: locale, vps o worker
# ------------------------------------------------------------------------------------------------
puo_amministrare() {
  [ "$(id -u)" -eq 0 ] && return 0
  c_e sudo || return 1
  sudo -n true 2>/dev/null && return 0
  id -nG | tr ' ' '\n' | grep -qxE 'sudo|wheel|admin'
}

if [ -z "$MODO" ]; then
  proposta="locale"
  if [ "$SISTEMA" = "Linux" ] && [ -d /run/systemd/system ] && puo_amministrare; then
    proposta="vps"
  fi
  if [ "$BIGLIETTO_NUOVO" -eq 1 ]; then
    if [ -f /etc/xbot/xbot.env ]; then MODO="vps"; else MODO="locale"; fi
  elif [ "$SOLO_VERSIONE" -eq 1 ]; then
    MODO="$proposta"
  else
    di "Modi: locale (questo computer, solo per te), vps (server raggiungibile da internet dietro HTTPS), worker (collega questo computer a un server)."
    chiedi MODO "Modo" "$proposta"
  fi
fi
case "$MODO" in locale|vps|worker) ;; *) errore "modo «$MODO» non valido: locale, vps o worker" ;; esac

if [ "$MODO" = "vps" ]; then
  [ "$SISTEMA" = "Linux" ] || errore "il modo vps vuole Linux"
  if [ ! -d /run/systemd/system ]; then
    if [ "$SENZA_SERVIZIO" -eq 1 ]; then avviso "systemd non è attivo: con --senza-servizio proseguo senza unità di sistema"
    else errore "systemd non è attivo su questa macchina (con --senza-servizio si installa senza servizio)"; fi
  fi
  if [ "$(id -u)" -ne 0 ]; then
    c_e sudo || errore "il modo vps vuole root o sudo"
    SUDO="sudo"
  fi
  UTENTE_SERVIZIO="xbot"
fi


# Cartelle e indirizzi.
PREFISSO_SCELTO=0; [ -z "$PREFISSO" ] || PREFISSO_SCELTO=1
[ -z "${S_PREFISSO_SCELTO:-}" ] || PREFISSO_SCELTO="$S_PREFISSO_SCELTO"
if [ "$MODO" = "vps" ]; then
  [ -n "$PREFISSO" ] || PREFISSO="/opt/xbot"
  [ -n "$DATI" ] || DATI="/var/lib/xbot"
  [ -n "$CONFIG" ] || CONFIG="/etc/xbot/xbot.env"
else
  [ -n "$PREFISSO" ] || PREFISSO="$HOME/.local/opt/xbot"
  [ -n "$DATI" ] || DATI="${XDG_DATA_HOME:-$HOME/.local/share}/xbot"
  if [ -z "$CONFIG" ]; then
    if [ "$PREFISSO_SCELTO" -eq 1 ]; then CONFIG="$PREFISSO/etc/xbot.env"
    else CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}/xbot/xbot.env"; fi
  fi
fi
WORKER_DIR="$DATI/worker"
[ "$MODO" != "vps" ] || PATH_SERVIZIO="$DATI/.local/bin:/usr/local/bin:/usr/bin:/bin"
ACCESSO="$DATI/accesso-iniziale.txt"
TOTP="$DATI/totp.txt"

if [ -z "$ASCOLTO" ]; then
  if [ -z "$PORTA" ] && [ -f "$CONFIG" ] && come_root grep -q '^XBOT_ASCOLTO=' "$CONFIG"; then
    ASCOLTO="$(come_root grep '^XBOT_ASCOLTO=' "$CONFIG" | tail -1 | cut -d= -f2- | tr -d '"')"
  elif [ "$MODO" = "vps" ] && [ -z "$PORTA" ] && [ "$BIGLIETTO_NUOVO" -eq 0 ]; then
    chiedi ASCOLTO "Indirizzo d'ascolto del server (quello verso cui punta il proxy)" "127.0.0.1:8787"
  else
    ASCOLTO="127.0.0.1:${PORTA:-8787}"
  fi
fi

# --- Solo un biglietto nuovo ---------------------------------------------------------------------
if [ "$BIGLIETTO_NUOVO" -eq 1 ]; then
  [ -f "$ACCESSO" ] || come_root test -f "$ACCESSO" || errore "nessuna installazione trovata in $DATI"
  come_root test -f "$TOTP" || errore "$TOTP non c'è più: chiedi il biglietto dall'app (Impostazioni, Computer, Collega)"
  B="$(come_root python3 -c "$PY_CHIAMA" "http://$ASCOLTO" "$ACCESSO" "$TOTP" biglietto)"
  PUB="$(come_root grep '^XBOT_URL_PUBBLICO=' "$CONFIG" 2>/dev/null | tail -1 | cut -d= -f2- | tr -d '"')"
  di "Sull'altro computer, dalla cartella del repository (vale 15 minuti):"
  di "  ./installa.sh --modo worker --url ${PUB:-http://$ASCOLTO} --biglietto $B"
  exit 0
fi

for d in "$HOME/.cargo/bin" /opt/homebrew/opt/rustup/bin; do
  case ":$PATH:" in *":$d:"*) ;; *) [ -d "$d" ] && PATH="$d:$PATH" ;; esac
done
export PATH

# --- Solo la versione ------------------------------------------------------------------------------
if [ "$SOLO_VERSIONE" -eq 1 ]; then
  INST="$(come_root cat "$DATI/versione.txt" 2>/dev/null || echo "nessuna (manca $DATI/versione.txt)")"
  REMOTA="$(git -C "$RADICE" ls-remote --tags --refs origin 'v*' 2>/dev/null | sed 's#.*refs/tags/##' | ultima_versione || true)"
  di "Installata: $INST"
  di "Sorgente:   $(versione_sorgente) ($RADICE)"
  di "Pubblicata: ${REMOTA:-non trovata (nessun tag v* su origin o rete assente)}"
  exit 0
fi

# --- Solo il modo di aggiornamento ------------------------------------------------------------------
if [ "$SOLO_MODO_AGG" -eq 1 ]; then
  applica_modo_aggiornamento
  salva_scelte
  exit 0
fi

# --- Aggiornamento, passo 1: sorgente nuovo, poi lo script nuovo fa il resto -----------------------
if [ "$AGGIORNA" -eq 1 ] && [ "$PASSO2" -eq 0 ]; then
  [ "$CONF_ESISTE" -eq 1 ] || errore "nessuna installazione registrata in questo clone ($FILE_SCELTE): installa prima con ./installa.sh"
  c_e git || errore "serve git"
  aggiorna_sorgente
fi
VERSIONE_NUOVA=""
if [ "$PASSO2" -eq 1 ]; then
  VERSIONE_NUOVA="$(versione_sorgente)"
  passo "Aggiornamento: da $(versione_di "$DA_REF") a $VERSIONE_NUOVA"
fi

if [ "$MODO" = "vps" ] && [ -z "$DOMINIO" ]; then
  chiedi DOMINIO "Dominio pubblico del server (vuoto: nessun proxy HTTPS per ora)" ""
fi
if [ "$MODO" = "worker" ]; then
  [ -n "$URL_SERVER" ] || chiedi URL_SERVER "Indirizzo del server (es. https://bot.example.org)" ""
  [ -n "$URL_SERVER" ] || errore "il modo worker vuole --url"
fi
if [ "$MODO" != "worker" ] && [ -z "$UTENTE" ]; then
  u="$(id -un)"; u="$(printf '%s' "${u:0:1}" | tr '[:lower:]' '[:upper:]')${u:1}"
  chiedi UTENTE "Il tuo nome nell'app" "$u"
fi

if [ "$MODO" = "vps" ] && [ -n "$DOMINIO" ]; then URL_PUBBLICO="https://$DOMINIO"
elif [ "$MODO" = "vps" ]; then URL_PUBBLICO="http://$ASCOLTO"
else URL_PUBBLICO="http://$ASCOLTO"; fi

di ""
di "Modo: $MODO · nome: $NOME · etichetta: $ETICHETTA"
di "Programmi: $PREFISSO · dati: $DATI · impostazioni: $CONFIG"
[ "$MODO" = "worker" ] || di "Ascolto: $ASCOLTO · indirizzo: $URL_PUBBLICO"
if interattivo && ! conferma "Procedo?"; then di "Fermato, nessuna modifica."; exit 0; fi

# ------------------------------------------------------------------------------------------------
# 3. Prerequisiti
# ------------------------------------------------------------------------------------------------
passo "Prerequisiti"
MANCA=""
c_e git || MANCA="$MANCA git"
c_e python3 || MANCA="$MANCA python3"
c_e curl || MANCA="$MANCA curl"
if ! c_e cc && ! c_e gcc && ! c_e clang; then MANCA="$MANCA compilatore-C"; fi
if [ -n "$MANCA" ]; then
  di "Mancano:$MANCA"
  if [ "$SISTEMA" = "Darwin" ]; then di "Su macOS: xcode-select --install (compilatore e git), python3 da python.org o Homebrew."
  else di "Su Debian e Ubuntu: sudo apt install build-essential git curl python3 (python3-venv serve solo con --con-voce)"; fi
  errore "installa quello che manca e rilancia"
fi

# Claude Code si registra quando si crea l'utente (solo alla prima installazione): si controlla
# qui, prima della compilazione, così un rilancio non costa la build.
if [ "$MODO" = "vps" ] && ! id "$UTENTE_SERVIZIO" >/dev/null 2>&1; then
  come_root useradd --system --user-group --home-dir "$DATI" --shell /usr/sbin/nologin "$UTENTE_SERVIZIO"
  come_root install -d -o "$UTENTE_SERVIZIO" -g "$UTENTE_SERVIZIO" -m 750 "$DATI" "$DATI/.local" "$DATI/.local/bin"
  di "Utente di sistema $UTENTE_SERVIZIO creato (casa $DATI)."
fi
if [ "$MODO" != "worker" ] && ! come_root test -f "$DATI/xbot.sqlite" && [ "$SENZA_CLAUDE" -eq 0 ] \
  && ! come_servizio sh -c 'command -v claude' >/dev/null 2>&1; then
  di "Claude Code (claude) non è nel PATH dell'utente del servizio: si registra solo alla prima installazione."
  if [ "$MODO" = "vps" ]; then
    di "  Installalo come $UTENTE_SERVIZIO: sudo -u $UTENTE_SERVIZIO -H bash -c 'curl -fsSL https://claude.ai/install.sh | bash'"
    di "  poi fai l'accesso: sudo -u $UTENTE_SERVIZIO -H $DATI/.local/bin/claude"
  else
    di "  Installalo (https://claude.ai/install.sh) e fai l'accesso con claude."
  fi
  if ! conferma "Continuo senza Claude Code?"; then
    errore "rilancia dopo aver installato claude, oppure con --senza-claude"
  fi
fi

for d in "$HOME/.cargo/bin" /opt/homebrew/opt/rustup/bin; do
  case ":$PATH:" in *":$d:"*) ;; *) [ -d "$d" ] && PATH="$d:$PATH" ;; esac
done
export PATH
if ! c_e cargo; then
  di "Rust (cargo) non c'è. Si installa con rustup: curl https://sh.rustup.rs -sSf | sh -s -- -y"
  if { [ "$SI" -eq 1 ] && [ "$INSTALLA_RUST" -eq 1 ]; } || conferma "Scarico e installo Rust ora?"; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    PATH="$HOME/.cargo/bin:$PATH"
  else
    errore "serve Rust: rilancia dopo averlo installato (o con --installa-rust)"
  fi
fi
RICHIESTA="$(sed -n 's/^rust-version *= *"\([0-9.]*\)".*/\1/p' "$RADICE/Cargo.toml" | head -1)"
# Con la shim di rustup ma senza toolchain predefinita cargo c'è e rustc fallisce.
if ! rustc --version >/dev/null 2>&1; then
  di "rustc non risponde: con rustup di solito manca la toolchain predefinita (rustup default stable)."
  if c_e rustup && { [ "$INSTALLA_RUST" -eq 1 ] || conferma "Imposto la toolchain stabile con rustup default stable?"; }; then
    rustup default stable
  else
    errore "lancia rustup default stable e rilancia (oppure rilancia con --installa-rust)"
  fi
fi
ATTUALE="$(rustc --version | awk '{print $2}')"
if [ -n "$RICHIESTA" ] && [ "$(printf '%s\n%s\n' "$RICHIESTA" "$ATTUALE" | sort -t. -k1,1n -k2,2n -k3,3n | head -1)" != "$RICHIESTA" ]; then
  di "Rust $ATTUALE, il progetto vuole $RICHIESTA."
  if c_e rustup && { [ "$INSTALLA_RUST" -eq 1 ] || conferma "Aggiorno con rustup update stable?"; }; then
    rustup update stable
  else
    errore "aggiorna Rust (rustup update stable) e rilancia"
  fi
fi
di "Rust: $(rustc --version)"

OPZIONALI=""
c_e ffmpeg || OPZIONALI="$OPZIONALI ffmpeg(voce)"
if [ "$MODO" = "vps" ]; then
  c_e caddy || OPZIONALI="$OPZIONALI caddy(proxy-HTTPS)"
  for p in Xvfb x11vnc ttyd tmux; do c_e "$p" || OPZIONALI="$OPZIONALI $p(computer)"; done
  c_e openbox || c_e fluxbox || OPZIONALI="$OPZIONALI openbox(computer)"
  c_e google-chrome || c_e google-chrome-stable || c_e chromium || c_e chromium-browser || OPZIONALI="$OPZIONALI chrome(computer)"
fi
if [ -n "$OPZIONALI" ]; then
  di "Facoltativi che mancano:$OPZIONALI"
  di "  La voce vuole ffmpeg; i computer degli assistenti (solo Linux) vogliono Xvfb, openbox, x11vnc, ttyd, tmux e Chrome."
  di "  Su Debian e Ubuntu: sudo apt install ffmpeg xvfb openbox x11vnc ttyd tmux (Chrome dal sito di Google). Non li installo io."
fi

# ------------------------------------------------------------------------------------------------
# 4. Compilazione
# ------------------------------------------------------------------------------------------------
passo "Compilazione (release)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$RADICE/target}"
if [ "$MODO" = "worker" ]; then PACCHETTI="-p xbot-worker"; else PACCHETTI="-p xbot-server -p xbot-worker"; fi
# shellcheck disable=SC2086
# Le righe «cargo:…» dei build script sono rumore per chi legge l'uscita; pipefail tiene l'esito.
set +e
(cd "$RADICE" && cargo build --release --locked $PACCHETTI 2>&1) | grep -v '^ *cargo:'
ESITO_CARGO="${PIPESTATUS[0]}"
set -e
if [ "$ESITO_CARGO" -ne 0 ]; then
  if [ "$PASSO2" -eq 1 ]; then
    torna_sorgente
    registra_aggiornamento "non installato" "compilazione non riuscita"
    errore "compilazione non riuscita: niente è stato sostituito, in opera resta $(versione_sorgente). Registro: $DATI/aggiornamenti.log"
  fi
  errore "compilazione non riuscita (vedi sopra)"
fi
USCITA="$CARGO_TARGET_DIR/release"
[ "$MODO" = "worker" ] || [ -x "$USCITA/xbot-server" ] || errore "xbot-server non è in $USCITA"
[ -x "$USCITA/xbot-worker" ] || errore "xbot-worker non è in $USCITA"

# ------------------------------------------------------------------------------------------------
# 5. File: binari, interfaccia, voce
# ------------------------------------------------------------------------------------------------
passo "Programmi in $PREFISSO"
if [ "$MODO" = "vps" ]; then
  if ! id "$UTENTE_SERVIZIO" >/dev/null 2>&1; then
    come_root useradd --system --user-group --home-dir "$DATI" --shell /usr/sbin/nologin "$UTENTE_SERVIZIO"
    di "Utente di sistema $UTENTE_SERVIZIO creato."
  fi
  come_root install -d -m 755 "$PREFISSO" "$PREFISSO/bin"
  come_root install -d -o "$UTENTE_SERVIZIO" -g "$UTENTE_SERVIZIO" -m 750 "$DATI" "$DATI/sessioni" "$DATI/log" "$DATI/backup" "$DATI/.local" "$DATI/.local/bin"
  come_root install -d -o root -g "$UTENTE_SERVIZIO" -m 750 "$(dirname "$CONFIG")"
else
  mkdir -p "$PREFISSO/bin" "$DATI/sessioni" "$DATI/log" "$DATI/backup" "$(dirname "$CONFIG")"
  chmod 700 "$DATI"
fi

# Copia del database prima di cambiare il binario (le migrazioni partono da sole all'avvio).
if come_root test -f "$DATI/xbot.sqlite"; then
  COPIA="$DATI/backup/xbot-$(date +%Y%m%d-%H%M%S).sqlite"
  if c_e sqlite3; then come_root sqlite3 "$DATI/xbot.sqlite" ".backup '$COPIA'"
  else come_root cp "$DATI/xbot.sqlite" "$COPIA"; fi
  di "Copia del database: $COPIA"
fi

[ "$PASSO2" -eq 0 ] || copia_precedente
if [ "$MODO" != "worker" ]; then
  come_root install -m 755 "$USCITA/xbot-server" "$PREFISSO/bin/xbot-server"
fi
come_root install -m 755 "$USCITA/xbot-worker" "$PREFISSO/bin/xbot-worker"

if [ "$MODO" != "worker" ]; then
  for c in web voce; do
    if c_e rsync; then come_root rsync -a --delete --exclude prove --exclude __pycache__ "$RADICE/$c/" "$PREFISSO/$c/"
    else come_root rm -rf "$PREFISSO/$c"; come_root cp -R "$RADICE/$c" "$PREFISSO/$c"; come_root rm -rf "$PREFISSO/$c/prove"; fi
  done
  come_root python3 -c "$PY_RINOMINA" "$PREFISSO/web" "$NOME"
  di "Interfaccia in $PREFISSO/web con il nome «$NOME»."
  if [ "$SISTEMA" = "Darwin" ]; then
    come_root mkdir -p "$PREFISSO/scarica"
    come_root install -m 755 "$USCITA/xbot-worker" "$PREFISSO/scarica/xbot-worker-macos"
  fi
  # Lo script di avvio carica il file d'ambiente (lo usano LaunchAgent, systemd utente e impianta).
  t="$(temporaneo)"
  cat >"$t" <<FINE
#!/bin/sh
# $SEGNO: carica il file d'ambiente e avvia xbot-server.
set -a
. "$CONFIG"
set +a
cd "$DATI" || exit 1
exec "$PREFISSO/bin/xbot-server" "\$@"
FINE
  come_root install -m 755 "$t" "$PREFISSO/bin/xbot-avvia"
fi

# Collegamenti in ~/.local/bin solo con il prefisso predefinito, e mai sopra file altrui.
if [ "$MODO" != "vps" ] && [ "$PREFISSO_SCELTO" -eq 0 ]; then
  mkdir -p "$HOME/.local/bin"
  for b in xbot-server xbot-worker; do
    [ -x "$PREFISSO/bin/$b" ] || continue
    dest="$HOME/.local/bin/$b"
    if [ -e "$dest" ] && [ ! -L "$dest" ]; then avviso "$dest esiste già e non è un collegamento: lo lascio com'è"
    else ln -sf "$PREFISSO/bin/$b" "$dest"; fi
  done
fi

# ------------------------------------------------------------------------------------------------
# 6. File d'ambiente e segreti
# ------------------------------------------------------------------------------------------------
if [ "$MODO" != "worker" ]; then
  passo "Impostazioni in $CONFIG"
  if ! come_root test -f "$CONFIG"; then
    if [ "$MODO" = "vps" ]; then
      come_root install -o root -g "$UTENTE_SERVIZIO" -m 640 "$RADICE/xbot.env.esempio" "$CONFIG"
    else
      (umask 077; cp "$RADICE/xbot.env.esempio" "$CONFIG")
    fi
    di "Creato da xbot.env.esempio."
  else
    di "Esiste già: aggiorno solo indirizzi e cartelle, i segreti restano."
  fi
  imposta XBOT_NOME "$NOME"
  imposta XBOT_ASCOLTO "$ASCOLTO"
  imposta XBOT_URL_PUBBLICO "$URL_PUBBLICO"
  imposta XBOT_DB "$DATI/xbot.sqlite"
  imposta XBOT_DATI "$DATI"
  imposta XBOT_SESSIONI "$DATI/sessioni"
  imposta XBOT_WEB "$PREFISSO/web"
  imposta XBOT_VOCE_CARTELLA "$PREFISSO/voce"
  imposta_se_manca XBOT_LOG "info"
  [ "$SISTEMA" != "Darwin" ] || imposta XBOT_SCARICA "$PREFISSO/scarica"
  if [ "$MODO" = "vps" ]; then
    imposta_se_manca XBOT_MACCHINA_NOME "vps"
    if [ -n "$DOMINIO" ]; then
      imposta XBOT_COOKIE_SICURO "1"
      imposta_se_manca XBOT_PROXY_FIDATO "127.0.0.1,::1"
    fi
  else
    imposta_se_manca XBOT_MACCHINA_NOME "locale"
  fi
  ha_var XBOT_SEGRETO_COOKIE || { imposta XBOT_SEGRETO_COOKIE "$(segreto_base64)"; di "Segreto dei cookie generato."; }
  ha_var XBOT_CASSAFORTE_CHIAVE || { imposta XBOT_CASSAFORTE_CHIAVE "$(segreto_base64)"; di "Chiave della cassaforte generata."; }
  if ! ha_var XBOT_VAPID_PRIVATA; then
    VAPID="$("$PREFISSO/bin/xbot-server" vapid)"
    imposta XBOT_VAPID_PRIVATA "$(printf '%s\n' "$VAPID" | sed -n 's/^XBOT_VAPID_PRIVATA=//p')"
    imposta XBOT_VAPID_PUBBLICA "$(printf '%s\n' "$VAPID" | sed -n 's/^XBOT_VAPID_PUBBLICA=//p')"
    VAPID=""
    di "Chiavi delle notifiche push generate."
  fi
  if [ -x "$PREFISSO/voce/venv/bin/python" ]; then
    imposta XBOT_VOCE_PYTHON "$PREFISSO/voce/venv/bin/python"
  elif [ "$CON_VOCE" -eq 1 ] || conferma "Creo il venv della voce con faster-whisper (scarica da PyPI)?"; then
    come_root python3 -m venv "$PREFISSO/voce/venv"
    come_root "$PREFISSO/voce/venv/bin/pip" install --quiet faster-whisper
    imposta XBOT_VOCE_PYTHON "$PREFISSO/voce/venv/bin/python"
    di "Voce: venv in $PREFISSO/voce/venv."
  fi
  if [ "$MODO" = "vps" ]; then come_root chown root:"$UTENTE_SERVIZIO" "$CONFIG"; come_root chmod 640 "$CONFIG"
  else chmod 600 "$CONFIG"; fi
fi

# ------------------------------------------------------------------------------------------------
# 7. Utente, password e secondo fattore (solo la prima volta)
# ------------------------------------------------------------------------------------------------
MOSTRA=0; interattivo && MOSTRA=1

if [ "$MODO" != "worker" ]; then
  passo "Utente"
  if come_root test -f "$DATI/xbot.sqlite"; then
    di "Il database c'è già: utente, password e TOTP restano quelli di prima."
  else
    PW=""
    if interattivo; then
      read -r -s -p "Password (almeno 8 caratteri, vuoto = generata): " PW || true; echo
      if [ -n "$PW" ]; then
        read -r -s -p "Ripeti la password: " PW2 || true; echo
        [ "$PW" = "$PW2" ] || errore "le due password non coincidono"
        PW2=""
      fi
    fi
    [ -n "$PW" ] || PW="$(password_nuova)"
    [ "${#PW}" -ge 8 ] || errore "la password vuole almeno 8 caratteri"
    XBOT_PASSWORD="$PW" come_servizio "$PREFISSO/bin/xbot-avvia" \
      impianta --nome "$UTENTE" --totp --totp-file "$TOTP" 2>&1 | grep -v 'INFO' || true
    come_root test -f "$TOTP" || errore "impianta non ha creato l'utente (vedi sopra)"
    t="$(temporaneo)"
    printf 'utente: %s\npassword: %s\nindirizzo: %s/app/\nscritto: %s\n' "$UTENTE" "$PW" "$URL_PUBBLICO" "$(ora)" >"$t"
    come_root install -m 600 "$t" "$ACCESSO"
    [ "$MODO" != "vps" ] || come_root chown root:root "$ACCESSO" "$TOTP"
    if [ "$MOSTRA" -eq 1 ]; then
      di ""
      di "Password (si mostra ora e basta, resta in $ACCESSO): $PW"
      di "Secondo fattore: aggiungi questo URL o il QR all'app di autenticazione."
      come_root cat "$TOTP"; echo
      if c_e qrencode; then come_root cat "$TOTP" | qrencode -t ANSIUTF8; fi
    else
      di "Password e URL del secondo fattore non si stampano: stanno in $ACCESSO e $TOTP (0600)."
    fi
    PW=""
  fi
fi

# ------------------------------------------------------------------------------------------------
# 8. Servizi
# ------------------------------------------------------------------------------------------------
porta_libera() {
  # Libera = nessuno accetta connessioni (un bind fallirebbe con i socket in TIME_WAIT).
  python3 -c 'import socket,sys
h,p=sys.argv[1].rsplit(":",1); h=h.strip("[]")
if h in ("0.0.0.0",""): h="127.0.0.1"
if h=="::": h="::1"
try: socket.create_connection((h,int(p)),timeout=2).close()
except OSError: sys.exit(0)
sys.exit(1)' "$1"
}

SERVIZIO_SERVER=""; SERVIZIO_WORKER=""

installa_servizio_server() {
  case "$MODO:$SISTEMA" in
    locale:Darwin)
      SERVIZIO_SERVER="$HOME/Library/LaunchAgents/$ETICHETTA.server.plist"
      nostro_o_assente "$SERVIZIO_SERVER" || errore "$SERVIZIO_SERVER esiste e non è di questo script: non lo tocco"
      riempi "$RADICE/ops/launchd/server.plist.modello" "$SERVIZIO_SERVER" 1 644
      launchctl bootout "gui/$(id -u)/$ETICHETTA.server" 2>/dev/null || true
      launchctl bootstrap "gui/$(id -u)" "$SERVIZIO_SERVER" ;;
    locale:Linux)
      SERVIZIO_SERVER="$HOME/.config/systemd/user/$SIGLA-server.service"
      nostro_o_assente "$SERVIZIO_SERVER" || errore "$SERVIZIO_SERVER esiste e non è di questo script: non lo tocco"
      controlla_systemd_utente || return 0
      riempi "$RADICE/ops/systemd/utente/server.service" "$SERVIZIO_SERVER" 0 644
      systemctl --user daemon-reload
      systemctl --user enable "$SIGLA-server.service" >/dev/null
      systemctl --user restart "$SIGLA-server.service" ;;
    vps:Linux)
      SERVIZIO_SERVER="/etc/systemd/system/$SIGLA.service"
      nostro_o_assente "$SERVIZIO_SERVER" || errore "$SERVIZIO_SERVER esiste e non è di questo script: non lo tocco"
      riempi "$RADICE/ops/systemd/xbot.service" "$SERVIZIO_SERVER" 0 644
      come_root systemctl daemon-reload
      come_root systemctl enable "$SIGLA.service" >/dev/null
      come_root systemctl restart "$SIGLA.service" ;;
  esac
}

installa_servizio_worker() {
  case "$SISTEMA" in
    Darwin)
      SERVIZIO_WORKER="$HOME/Library/LaunchAgents/$ETICHETTA.worker.plist"
      nostro_o_assente "$SERVIZIO_WORKER" || { avviso "$SERVIZIO_WORKER esiste e non è di questo script: non lo tocco"; return; }
      riempi "$RADICE/ops/launchd/worker.plist.modello" "$SERVIZIO_WORKER" 1 644
      launchctl bootout "gui/$(id -u)/$ETICHETTA.worker" 2>/dev/null || true
      launchctl bootstrap "gui/$(id -u)" "$SERVIZIO_WORKER" ;;
    Linux)
      SERVIZIO_WORKER="$HOME/.config/systemd/user/$SIGLA-worker.service"
      nostro_o_assente "$SERVIZIO_WORKER" || { avviso "$SERVIZIO_WORKER esiste e non è di questo script: non lo tocco"; return; }
      controlla_systemd_utente || return 0
      riempi "$RADICE/ops/systemd/utente/worker.service" "$SERVIZIO_WORKER" 0 644
      systemctl --user daemon-reload
      systemctl --user enable "$SIGLA-worker.service" >/dev/null
      systemctl --user restart "$SIGLA-worker.service" ;;
  esac
}

if [ "$MODO" != "worker" ]; then
  passo "Servizio del server"
  GIA_SU=0; salute >/dev/null && GIA_SU=1
  if [ "$GIA_SU" -eq 0 ] && ! porta_libera "$ASCOLTO"; then
    errore "l'indirizzo $ASCOLTO è occupato da un altro programma: scegline un altro con --porta o --ascolto"
  fi
  SALTA_VERIFICA=0
  if [ "$SENZA_SERVIZIO" -eq 1 ] && [ "$PASSO2" -eq 1 ] && [ "$GIA_SU" -eq 1 ]; then
    di "Un server avviato a mano risponde su $ASCOLTO: file aggiornati, riavvialo tu ($PREFISSO/bin/xbot-avvia) per usare la versione nuova."
    SALTA_VERIFICA=1
  elif [ "$SENZA_SERVIZIO" -eq 1 ]; then
    di "Niente servizio (--senza-servizio): avvio temporaneo per la verifica."
    if [ "$GIA_SU" -eq 1 ]; then
      di "Un server risponde già su $ASCOLTO: lo uso per la verifica."
    else
      SOSTITUISCI=1 come_servizio "$PREFISSO/bin/xbot-avvia" >>"$DATI/log/avvio-prova.log" 2>&1 &
      PID_PROVA=$!
    fi
  else
    installa_servizio_server
    di "Servizio: $SERVIZIO_SERVER"
  fi
fi

# ------------------------------------------------------------------------------------------------
# 9. Verifica, primo assistente, lavoratore
# ------------------------------------------------------------------------------------------------
if [ "$MODO" != "worker" ] && [ "${SALTA_VERIFICA:-0}" -eq 0 ]; then
  passo "Verifica"
  if aspetta_salute; then
    di "salute: $(salute)"
  elif [ "$PASSO2" -eq 1 ]; then
    ripristina_aggiornamento "il server nuovo non risponde su http://$ASCOLTO/api/v1/salute entro 30 secondi"
  else
    errore "il server non risponde su http://$ASCOLTO/api/v1/salute (log in $DATI/log/)"
  fi

  if come_root test -f "$DATI/.installa-assistente"; then
    di "Primo assistente: già fatto in un giro precedente."
  elif come_root test -f "$TOTP"; then
    esito="$(come_root python3 -c "$PY_CHIAMA" "http://$ASCOLTO" "$ACCESSO" "$TOTP" assistente "$NOME")"
    di "Primo assistente «$NOME»: $esito."
    t="$(temporaneo)"; printf '%s\n' "$(ora)" >"$t"; come_root install -m 600 "$t" "$DATI/.installa-assistente"
  else
    avviso "$TOTP non c'è: il primo assistente va creato dall'app"
  fi

  if [ "$MODO" = "locale" ] && [ "$SENZA_WORKER" -eq 0 ]; then
    if [ -f "$WORKER_DIR/credenziale.json" ]; then
      di "Lavoratore di questo computer: già collegato."
    elif [ -f "$TOTP" ]; then
      B="$(python3 -c "$PY_CHIAMA" "http://$ASCOLTO" "$ACCESSO" "$TOTP" biglietto)"
      mkdir -p "$WORKER_DIR"; chmod 700 "$WORKER_DIR"
      # Il biglietto passa da stdin («-»), così non compare in ps.
      printf '%s\n' "$B" | "$PREFISSO/bin/xbot-worker" enroll "http://$ASCOLTO" - --nome "$(hostname -s 2>/dev/null || echo questo-pc)" --cartella "$WORKER_DIR"
      B=""
    fi
    if [ -f "$WORKER_DIR/credenziale.json" ] && [ "$SENZA_SERVIZIO" -eq 0 ]; then
      installa_servizio_worker
      di "Lavoratore: ${SERVIZIO_WORKER:-servizio non installato}"
    fi
  fi
fi

if [ "$MODO" = "worker" ]; then
  passo "Collegamento al server $URL_SERVER"
  mkdir -p "$WORKER_DIR"; chmod 700 "$WORKER_DIR"
  if [ -f "$WORKER_DIR/credenziale.json" ]; then
    di "Già collegato (credenziale in $WORKER_DIR)."
  else
    [ -n "$BIGLIETTO" ] || BIGLIETTO="${XBOT_BIGLIETTO:-}"
    [ -n "$BIGLIETTO" ] || chiedi BIGLIETTO "Biglietto (dall'app: Impostazioni, Computer, Collega)" ""
    [ -n "$BIGLIETTO" ] || errore "serve il biglietto (--biglietto)"
    printf '%s\n' "$BIGLIETTO" | "$PREFISSO/bin/xbot-worker" enroll "$URL_SERVER" - --nome "$(hostname -s 2>/dev/null || echo computer)" --cartella "$WORKER_DIR"
  fi
  if [ "$SENZA_SERVIZIO" -eq 0 ]; then installa_servizio_worker; di "Servizio: $SERVIZIO_WORKER"; fi
  "$PREFISSO/bin/xbot-worker" client 2>/dev/null | python3 -c 'import json,sys
try: print("Client trovati: " + (", ".join(p.get("id", "?") for p in json.load(sys.stdin)) or "nessuno"))
except Exception: pass' || true
fi

if [ -n "$PID_PROVA" ]; then
  kill "$PID_PROVA" 2>/dev/null || true
  wait "$PID_PROVA" 2>/dev/null || true
  PID_PROVA=""
  if salute >/dev/null; then avviso "il server temporaneo risponde ancora su $ASCOLTO"
  else di "Server temporaneo fermato."; fi
fi

# Versione in opera, scelte per --aggiorna, modo di aggiornamento.
passo "Versione e aggiornamenti"
if [ "$MODO" != "worker" ]; then
  imposta XBOT_SORGENTE "$RADICE"
  imposta XBOT_VERSIONE_FILE "$DATI/versione.txt"
  REPO_ORIGIN="$(git -C "$RADICE" remote get-url origin 2>/dev/null | sed -nE 's#.*github\.com[:/]([^/]+/[^/.]+)(\.git)?$#\1#p')"
  [ -z "$REPO_ORIGIN" ] || imposta_se_manca XBOT_REPO "$REPO_ORIGIN"
fi
come_root mkdir -p "$DATI"
scrivi_versione
applica_modo_aggiornamento
salva_scelte
di "Versione: $(come_root cat "$DATI/versione.txt")"
[ "$PASSO2" -eq 0 ] || registra_aggiornamento "ok"

# ------------------------------------------------------------------------------------------------
# 10. Proxy HTTPS (solo vps con dominio): si stampa, non si tocca un Caddy esistente
# ------------------------------------------------------------------------------------------------
if [ "$MODO" = "vps" ] && [ -n "$DOMINIO" ]; then
  passo "Proxy HTTPS (Caddy)"
  BLOCCO="$(dirname "$CONFIG")/Caddyfile.$SIGLA"
  riempi "$RADICE/ops/caddy/Caddyfile.esempio" "$BLOCCO" 0 644
  di "Blocco per $DOMINIO scritto in $BLOCCO:"
  come_root cat "$BLOCCO"
  di "Mettilo in coda al tuo Caddyfile e ricarica Caddy (caddy reload). Il DNS di $DOMINIO deve puntare qui e le porte 80 e 443 devono essere aperte."
  di "Se Caddy gira in un contenitore, il server deve ascoltare su un indirizzo che il contenitore raggiunge (--ascolto) e XBOT_PROXY_FIDATO deve contenere la rete del contenitore."
fi

# ------------------------------------------------------------------------------------------------
# 11. Riepilogo
# ------------------------------------------------------------------------------------------------
passo "Fatto ($(ora))"
if [ "$MODO" != "worker" ]; then
  di "Apri: $URL_PUBBLICO/app/"
  di "Utente: $UTENTE (password e indirizzo in $ACCESSO, permessi 0600)"
  come_root test -f "$TOTP" && di "Secondo fattore: $TOTP (0600). Aggiungilo all'app di autenticazione, poi cancellalo."
  di "Provider trovati nel PATH del servizio:"
  for p in claude agent cursor-agent grok codex gemini; do
    if PATH="$PATH_SERVIZIO" command -v "$p" >/dev/null 2>&1; then di "  $p"; fi
  done
  di "  L'accesso ai client (claude, agent, grok) lo fai tu con il tuo abbonamento, nel terminale come utente del servizio o dall'app (Impostazioni, Modelli e quote)."
  di "Collegare un altro computer: ./installa.sh --biglietto-nuovo qui, poi sull'altro computer ./installa.sh --modo worker --url $URL_PUBBLICO --biglietto <biglietto>."
  [ "$MODO" != "vps" ] || di "Log: journalctl -u $SIGLA -f"
  [ "$SENZA_SERVIZIO" -eq 0 ] || di "Per avviarlo a mano: $PREFISSO/bin/xbot-avvia"
else
  di "Questo computer è collegato a $URL_SERVER. Credenziale in $WORKER_DIR (0600)."
fi
