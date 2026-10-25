#!/usr/bin/env python3
"""Normalizzazione «tutto per intero» per la voce di XBot (Fase 2.D).

Legge il testo da stdin e stampa il testo come va detto: niente formattazione, numeri, orari,
date, importi, percentuali, unità e sigle in parole italiane (regola del progetto del 2026-10-05).
Le stesse regole della voce del Mac, riscritte qui da zero (la voce del Mac vive in un
programma di terzi con licenza AGPL, che non si copia); i dati vengono da `sigle.json`
accanto a questo file.

Prova: echo "Alle 16:30 la VPS ha fatto +70% (1.250,50 €) il 2026-10-24." | python3 normalizza.py
"""
import datetime
import json
import os
import re
import sys

GIORNI = ["lunedì", "martedì", "mercoledì", "giovedì", "venerdì", "sabato", "domenica"]
ANNO = int(os.environ.get("XBOT_ANNO") or datetime.date.today().year)  # l'anno in corso non si dice
QUI = os.path.dirname(os.path.abspath(__file__))
MESI = ["gennaio", "febbraio", "marzo", "aprile", "maggio", "giugno", "luglio", "agosto",
        "settembre", "ottobre", "novembre", "dicembre"]
UNITA = ["zero", "uno", "due", "tre", "quattro", "cinque", "sei", "sette", "otto", "nove", "dieci",
         "undici", "dodici", "tredici", "quattordici", "quindici", "sedici", "diciassette",
         "diciotto", "diciannove"]
DECINE = {2: "venti", 3: "trenta", 4: "quaranta", 5: "cinquanta", 6: "sessanta", 7: "settanta",
          8: "ottanta", 9: "novanta"}
ORDINALI = ["", "primo", "secondo", "terzo", "quarto", "quinto", "sesto", "settimo", "ottavo",
            "nono", "decimo"]


def dati():
    """sigle.json, più sigle.locali.json (facoltativo, non versionato) le cui voci vincono."""
    d = {}
    for nome in ("sigle.json", "sigle.locali.json"):
        try:
            with open(os.path.join(QUI, nome), encoding="utf-8") as f:
                altro = json.load(f)
        except (OSError, ValueError):
            continue
        for k, v in altro.items():
            if isinstance(v, dict):
                d.setdefault(k, {}).update(v)
    return {k: d.get(k) or {} for k in ("frasi", "abbreviazioni", "sigle", "inglesi", "nomi", "unita", "siti")}


# ---------------------------------------------------------------- numeri in parole
def _sotto100(n):
    if n < 20:
        return UNITA[n]
    d, u = divmod(n, 10)
    t = DECINE[d]
    if u in (1, 8):
        t = t[:-1]
    return t + (UNITA[u] if u else "")


def _sotto1000(n):
    c, r = divmod(n, 100)
    testa = "" if c == 0 else ("cento" if c == 1 else UNITA[c] + "cento")
    if testa and r // 10 == 8:
        testa = testa[:-1]
    return testa + (_sotto100(r) if r else "")


def parole(n):
    """Un intero in parole italiane (con «tré» in fondo ai composti)."""
    if n < 0:
        return "meno " + parole(-n)
    if n == 0:
        return "zero"
    pezzi = []
    for valore, uno, molti in ((10**9, "un miliardo", "miliardi"), (10**6, "un milione", "milioni")):
        q, n = divmod(n, valore)
        if q:
            pezzi.append(uno if q == 1 else f"{parole(q)} {molti}")
    q, n = divmod(n, 1000)
    testo = ""
    if q:
        testo = "mille" if q == 1 else _sotto1000(q) + "mila"
    if n:
        testo += _sotto1000(n)
    if testo:
        pezzi.append(testo)
    fuori = " ".join(pezzi)
    return re.sub(r"(?<=\w)tre\b", "tré", fuori)


def cifre(s):
    return " ".join(parole(int(c)) for c in s)


def intero(s):
    s = s.replace(".", "")
    if len(s) > 1 and s.startswith("0"):
        return cifre(s)
    return parole(int(s))


def decimali(s):
    zeri = len(s) - len(s.lstrip("0"))
    resto = s.lstrip("0")
    pezzi = ["zero"] * zeri
    if resto:
        pezzi.append(parole(int(resto)) if len(resto) <= 3 else cifre(resto))
    return " ".join(pezzi)


def numero(i, d=None):
    t = intero(i)
    if d:
        t += " virgola " + decimali(d)
    return t


def ordinale(n, femminile=False):
    if n <= 10:
        p = ORDINALI[n]
    else:
        c = parole(n).replace("tré", "tre")
        p = c + "esimo" if c.endswith(("tre", "sei")) else c[:-1] + "esimo"
    return p[:-1] + "a" if femminile else p


# ---------------------------------------------------------------- articoli
_PREP = {"del": ("di", "s"), "della": ("di", "s"), "dello": ("di", "s"), "dell'": ("di", "s"),
         "dei": ("di", "p"), "degli": ("di", "p"), "delle": ("di", "p"),
         "al": ("a", "s"), "alla": ("a", "s"), "allo": ("a", "s"), "all'": ("a", "s"),
         "ai": ("a", "p"), "agli": ("a", "p"), "alle": ("a", "p"),
         "dal": ("da", "s"), "dalla": ("da", "s"), "dallo": ("da", "s"), "dall'": ("da", "s"),
         "dai": ("da", "p"), "dagli": ("da", "p"), "dalle": ("da", "p"),
         "nel": ("in", "s"), "nella": ("in", "s"), "nello": ("in", "s"), "nell'": ("in", "s"),
         "nei": ("in", "p"), "negli": ("in", "p"), "nelle": ("in", "p"),
         "sul": ("su", "s"), "sulla": ("su", "s"), "sullo": ("su", "s"), "sull'": ("su", "s"),
         "sui": ("su", "p"), "sugli": ("su", "p"), "sulle": ("su", "p"),
         "il": ("", "s"), "la": ("", "s"), "lo": ("", "s"), "l'": ("", "s"),
         "i": ("", "p"), "gli": ("", "p"), "le": ("", "p"),
         "un": ("un", "s"), "uno": ("un", "s"), "una": ("un", "s"), "un'": ("un", "s")}
_FUSE = {"di": "de", "a": "a", "da": "da", "in": "ne", "su": "su"}


def articolo(prep, genere, num, parola):
    w = parola.lower()
    vocale = w[:1] in "aeiouàèéìòù"
    impura = bool(re.match(r"(s[^aeiou]|z|gn|ps|x|y)", w))
    if prep == "un":
        if genere == "f":
            return "un'" if vocale else "una "
        return "uno " if impura else "un "
    if genere == "f":
        base = "le" if num == "p" else ("l'" if vocale else "la")
    elif num == "p":
        base = "gli" if vocale or impura else "i"
    else:
        base = "l'" if vocale else ("lo" if impura else "il")
    if prep:
        if base == "il":
            base = {"di": "del", "a": "al", "da": "dal", "in": "nel", "su": "sul"}[prep]
        elif base == "i":
            base = {"di": "dei", "a": "ai", "da": "dai", "in": "nei", "su": "sui"}[prep]
        else:
            base = _FUSE[prep] + "l" + base
    return base if base.endswith("'") else base + " "


_ART = r"(?:\b(?P<art>(?i:" + "|".join(sorted((re.escape(a) for a in _PREP), key=len, reverse=True)) + r"))\s?)?"


# ---------------------------------------------------------------- passaggi
def via_markdown(t):
    t = re.sub(r"```.*?```", " ", t, flags=re.S)
    t = re.sub(r"!?\[([^\]]*)\]\([^)]*\)", r"\1", t)
    righe = []
    for r in t.splitlines():
        r = re.sub(r"^\s*(?:#{1,6}|>+|[-*+•]|\d+[.)])\s+", "", r)
        r = re.sub(r"^\s*\|?\s*:?-{3,}.*$", "", r).strip().strip("|").strip()
        if r:
            righe.append(r)
    if len(righe) > 1:  # fine riga = pausa
        righe = [r if r[-1] in ".!?:;," else r + "." for r in righe]
    return " ".join(righe)


# ---------------------------------------------------------------- come parla una persona
_PREP_SITO = {"su": "sul sito", "da": "dal sito", "di": "del sito", "in": "nel sito", "a": "al sito",
              "con": "con il sito", "per": "per il sito", "sul": "sul sito", "dal": "dal sito"}
_EST = ("md|js|mjs|ts|tsx|py|rs|json|toml|yaml|yml|sh|html|css|csv|xlsx|docx|pdf|txt|png|jpg|jpeg|gif|svg|webp|wav|mp3|"
        "sqlite|db|log|env|lock|service|timer|conf|cfg|sql|xml|zip|ini|plist")
_CLI = ("cargo|git|npm|npx|node|python|python3|pip|pip3|ssh|scp|rsync|curl|wget|ls|cd|rm|mv|cp|mkdir|sudo|docker|"
        "systemctl|journalctl|brew|adb|make|grep|cat|chmod|kill|pkill|tar|ffmpeg|claude|gh")


def _nome_sito(host, siti):
    host = host.lower().strip(".")
    if host in siti:
        return siti[host]
    parti = [p for p in host.split(".") if p and p != "www"]
    if len(parti) >= 2:
        parti = parti[:-1]  # via il dominio di primo livello
    nome = parti[-1] if len(parti) < 3 else parti[0]
    return nome.replace("-", " ")


def _nome_file(seg):
    seg = re.sub(r"\.(?:%s)$" % _EST, "", seg, flags=re.I)
    seg = re.sub(r"[_]+", " ", seg)
    return seg.strip()


def _dopo_prep(t, inizio):
    m = re.search(r"(\b\w+)\s$", t[:inizio])
    return m.group(1).lower() if m else ""


_ESTRE = re.compile(r"\.(?:%s)\b" % _EST, re.I)
_SEG = re.compile(r"[\w.@+()\[\]-]+(?: [\w.@+()\[\]-]+| - )*")


def _percorsi(t):
    """Percorsi assoluti (anche con spazi nei nomi) e relativi: resta il nome del file o della cartella.
    Scansione a mano, senza espressioni annidate: tempo lineare."""
    out, i, n = [], 0, len(t)
    inizio = re.compile(r"(?<![\w/:.])(?:~|\.{1,2})?/(?=[\w.@(\[~])|(?<![\w/.])[A-Za-z_][\w.-]*/(?=[\w.])")
    while True:
        m = inizio.search(t, i)
        if not m:
            out.append(t[i:])
            break
        out.append(t[i:m.start()])
        j, spazi, ultimo, fine = m.end(), False, "", m.end()
        while True:  # segmenti separati da /
            s = _SEG.match(t, j)
            if not s:
                break
            testo = s.group(0)
            k = s.end()
            if k < n and t[k] == "/" and k + 1 < n and re.match(r"[\w.@(\[~]", t[k + 1]):
                spazi = spazi or " " in testo
                j, ultimo, fine = k + 1, testo, k + 1
                continue
            # ultimo segmento: file (taglio all'estensione) o cartella
            e = _ESTRE.search(t, j, min(n, j + 100))
            if e and "\n" not in t[j:e.end()] and (e.start() == j or not re.search(r"[,;:!?]\s", t[j:e.start()])):
                ultimo, fine = t[j:e.end()], e.end()
            else:
                ultimo, fine = testo, k
                if spazi:  # nomi con spazi: «Progetti Esempio»
                    extra = re.match(r"(?: [A-Z][\w-]*){1,2}", t[k:])
                    if extra and " " not in testo:
                        ultimo, fine = testo + extra.group(0), k + extra.end()
            break
        if not ultimo:
            out.append(t[m.start():m.end()])
            i = m.end()
            continue
        nome = _nome_file(ultimo)
        file = bool(_ESTRE.search(ultimo)) or bool(re.search(r"\.\w{1,5}$", ultimo))
        prima = "".join(out)[-14:].lower()
        if file or re.search(r"(cartella|directory|\bin|\bsu|\bda|\bnel|\bnella)\s$", prima):
            out.append(nome)
        else:
            out.append("la cartella " + nome)
        i = fine
    return "".join(out)


def parlato(t, siti):
    """Toglie ciò che una persona non direbbe mai: indirizzi, percorsi, hash, flag, token."""
    t = re.sub(r"file:/{2,3}", "/", t)
    # segnalazioni e richieste di modifica su GitHub: si dice cos'è, non l'indirizzo
    t = re.sub(r"<?https?://github\.com/[\w.-]+/([\w.-]+)/(issues|pull)/(\d+)[^\s<>()]*[^\s<>().,;:!?]?>?",
               lambda m: ("una segnalazione" if m.group(2) == "issues" else "una richiesta di modifica") + f" di {m.group(1).replace('-', ' ')} su GitHub, la numero {m.group(3)}", t)
    # indirizzi web: solo il nome del sito
    def web(m):
        host = re.match(r"(?:https?://)?(?:[^@/\s]+@)?([^/:?#\s]+)", m.group(0)).group(1)
        return "il sito " + _nome_sito(host, siti)
    t = re.sub(r"<?(?:https?://|www\.)[^\s<>()\]]*[^\s<>()\].,;:!?]>?", web, t)
    t = re.sub(r"(?<![\w@/.])(?:[a-z0-9-]+\.)+(?:com|it|org|net|io|ai|dev|app|cloud|eu|co|me)(?:/[^\s<>()]*[^\s<>().,;:!?])?(?![\w@])",
               lambda m: "il sito " + _nome_sito(m.group(0).split("/")[0], siti), t, flags=re.I)
    # posta elettronica
    t = re.sub(r"\b([\w.+-]+)@([\w-]+(?:\.[\w-]+)+)\b",
               lambda m: "l'indirizzo email di " + re.sub(r"[._+-]+", " ", m.group(1)), t)
    # preposizione + «il sito» / «l'indirizzo» -> articolata
    def art(m):
        p = m.group(1).lower()
        sito = m.group(2) == "il sito"
        if sito:
            return _PREP_SITO.get(p, p + " il sito") + " "
        return {"su": "sull'", "da": "dall'", "di": "dell'", "in": "nell'", "a": "all'"}.get(p, p + " l'") + "indirizzo email "
    t = re.sub(r"\b(su|da|di|in|a|con|per)\s+(il sito |l'indirizzo email )", lambda m: art(re.match(r"(\w+)\s+(il sito|l'indirizzo email)", m.group(0))), t, flags=re.I)

    t = _percorsi(t)
    # nomi di file isolati: senza estensione
    t = re.sub(r"(?<![\w/.])([\w-]+)\.(?:%s)\b" % _EST, lambda m: _nome_file(m.group(1)), t, flags=re.I)

    # comandi tra apici inversi: «il comando cargo build»
    def cmd(m):
        parole_ = [w for w in m.group(1).split() if not w.startswith("-")]
        if parole_ and parole_[0] in _CLI.split("|"):
            return "il comando " + " ".join(parole_[:2])
        return m.group(1)
    t = re.sub(r"`([^`\n]+)`", cmd, t)
    t = re.sub(r"(?<!\w)--?[A-Za-z][\w-]*(?=\s|$|[.,;:])", "", t)  # flag da riga di comando

    # hash, token, indirizzi di rete, porte, versioni, numeri di richiesta
    t = re.sub(r"\b(?:(commit|hash)\s+)?(?=[0-9a-f]*\d)(?=[0-9a-f]*[a-f])[0-9a-f]{7,40}\b",
               lambda m: (m.group(1) or "un commit") if not m.group(1) else m.group(1), t)
    t = re.sub(r"\b(?=[\w-]*\d)(?=[\w-]*[A-Za-z])[\w-]{24,}\b", "un codice segreto", t)
    t = re.sub(r"\b(?:127\.0\.0\.1|localhost|0\.0\.0\.0)(?::\d+)?", "l'indirizzo locale", t)
    t = re.sub(r"\b\d{1,3}(?:\.\d{1,3}){3}(?::\d+)?\b", "un indirizzo di rete", t)
    cifra = ["zero", "uno", "due", "tre", "quattro", "cinque", "sei", "sette", "otto", "nove"]
    t = re.sub(r"\b([Pp]orta)\s+(\d{3,5})\b", lambda m: m.group(1) + " " + " ".join(cifra[int(c)] for c in m.group(2)), t)

    def versione(m):
        pezzi = m.group(0).split(".")
        tenuti = []
        for x in pezzi:
            if len(x) >= 4:
                break
            tenuti.append(x)
        return " punto ".join(tenuti[:3]) if len(tenuti) >= 2 else tenuti[0] if tenuti else pezzi[0]
    t = re.sub(r"\b\d{1,3}(?:\.\d+){2,3}\b(?!\.\d)", versione, t)
    t = re.sub(r"#(\d+)\b", r"numero \1", t)
    t = re.sub(r"(?:^|(?<=[:;.!?] ))(\d{1,2})\)\s+", lambda m: ordinale(int(m.group(1))) + ", ", t)
    t = re.sub(r"\(\s*\)", "", t)
    t = re.sub(r"\b(su|da|di|in|a) l'", lambda m: {"su": "sull'", "da": "dall'", "di": "dell'", "in": "nell'", "a": "all'"}[m.group(1)], t)
    return t


def sostituzioni(t, voci, maiuscole=True):
    for k in sorted(voci, key=len, reverse=True):
        v = voci[k]
        if not v:
            continue
        fine = r"(?![\w])" if k[-1:].isalnum() else ""
        t = re.sub(r"(?<![\w])" + re.escape(k) + fine, lambda m, v=v: v, t,
                   flags=0 if maiuscole else re.I)
    return t


def _giorno(stringa, inizio, a, me, g):
    """«martedì » se la data è completa e davanti non c'è già un giorno della settimana."""
    prima = re.search(r"(\w+)\s$", stringa[:inizio])
    if prima and prima.group(1).lower() in GIORNI:
        return ""
    try:
        return GIORNI[datetime.date(a, me, g).weekday()] + " "
    except ValueError:
        return ""


def date(t):
    def iso(m):
        a, me, g = int(m.group(1)), int(m.group(2)), int(m.group(3))
        if not (1 <= me <= 12 and 1 <= g <= 31):
            return m.group(0)
        return _giorno(m.string, m.start(), a, me, g) + f"{'primo' if g == 1 else parole(g)} {MESI[me - 1]}" + ("" if a == ANNO else f" {parole(a)}")
    t = re.sub(r"(?<!\d)(\d{4})-(\d{1,2})-(\d{1,2})(?!\d)", iso, t)

    def barra(m):
        g, me, a = int(m.group(1)), int(m.group(2)), m.group(3)
        if not (1 <= me <= 12 and 1 <= g <= 31):
            return m.group(0)
        anno = (int(a) + 2000 if len(a) == 2 else int(a))
        return _giorno(m.string, m.start(), anno, me, g) + f"{'primo' if g == 1 else parole(g)} {MESI[me - 1]}" + ("" if anno == ANNO else f" {parole(anno)}")
    return re.sub(r"(?<![\d/])(\d{1,2})/(\d{1,2})/(\d{4}|\d{2})(?![\d/])", barra, t)


def orari(t):
    def ora(h, mi):
        if h in (0, 24):
            nome = "mezzanotte"
        elif h == 12:
            nome = "mezzogiorno"
        else:
            nome = parole(h)
        return nome + {0: "", 15: " e un quarto", 30: " e mezza"}.get(mi, f" e {parole(mi)}")

    def una(m):
        return una_(m) + (" del mattino" if 4 <= int(m.group("h")) <= 11 else " di sera" if 20 <= int(m.group("h")) <= 23 else "")

    def una_(m):
        pre, h, mi = m.group("pre"), int(m.group("h")), int(m.group("m"))
        if pre and h == 1 and pre.lower() in ("alle", "le", "dalle"):
            pre = {"alle": "all'", "le": "l'", "dalle": "dall'"}[pre.lower()]
            return pre + ora(h, mi).replace("uno", "una")
        if pre and h in (0, 12, 24) and pre.lower() in ("alle", "le", "dalle"):
            pre = {"alle": "a", "le": "", "dalle": "da"}[pre.lower()]
        return (pre + " " if pre else "") + ora(h, mi)
    def intervallo(m):
        dalle = "Dalle" if (m.group("pre") or "")[:1] == "D" else "dalle"
        h1, h2 = int(m.group(2)), int(m.group(4))
        da = (dalle[:2] if h1 in (0, 12, 24) else ("dall'" if h1 == 1 else dalle + " "))
        a = ("a" if h2 in (0, 12, 24) else ("all'" if h2 == 1 else "alle "))
        if h1 in (0, 12, 24):
            da += " "
        if h2 in (0, 12, 24):
            a += " "
        uno = lambda h, mi: ora(h, mi).replace("uno", "una") if h == 1 else ora(h, mi)
        return f"{da}{uno(h1, int(m.group(3)))} {a}{uno(h2, int(m.group(5)))}"
    t = re.sub(r"(?:(?P<pre>\b[Dd]alle)\s)?(?<![\w:.,])([01]?\d|2[0-4]):([0-5]\d)\s?[-–]\s?([01]?\d|2[0-4]):([0-5]\d)(?![\d:])",
               intervallo, t)
    return re.sub(r"(?:(?P<pre>\b(?:alle|dalle|le|ore))\s)?(?<![\d.,:])(?P<h>[01]?\d|2[0-4]):(?P<m>[0-5]\d)(?![\d:])",
                  una, t, flags=re.I)


def importi(t):
    def euro(m):
        n = (m.group("a") or m.group("c")).replace(".", "")
        cent = m.group("b") or m.group("d")
        s = f"{parole(int(n))} euro"
        if cent and int(cent):
            s += f" e {parole(int(cent))} centesimi"
        return s
    return re.sub(r"€\s?(?P<a>\d{1,3}(?:\.\d{3})+|\d+)(?:,(?P<b>\d{2}))?"
                  r"|(?P<c>\d{1,3}(?:\.\d{3})+|\d+)(?:,(?P<d>\d{2}))?\s?(?:€|euro\b)", euro, t, flags=re.I)


def percentuali(t):
    segni = {"+": "più ", "-": "meno ", "−": "meno "}
    return re.sub(r"(?:(?<![\w\d])(?P<s>[+\-−])\s?)?(?<![\d.,])(?P<n>\d+)(?:[.,](?P<d>\d+))?\s?%",
                  lambda m: segni.get(m.group("s") or "", "") + numero(m.group("n"), m.group("d")) + " per cento", t)


def ordinali(t):
    return re.sub(r"(?<![\d.,])(\d{1,3})([°ºª])(?!C)", lambda m: ordinale(int(m.group(1)), m.group(2) == "ª"), t)


def unita(t, voci):
    if not voci:
        return t
    chiavi = "|".join(sorted((re.escape(k) for k in voci), key=len, reverse=True))

    def u(m):
        uno, molti = voci[m.group("u")]
        if not m.group("d") and m.group("n").replace(".", "") == "1":
            return uno
        return f"{numero(m.group('n'), m.group('d'))} {molti}"
    return re.sub(r"(?<![\d.,])(?P<n>\d{1,3}(?:\.\d{3})+|\d+)(?:,(?P<d>\d+))?\s?(?P<u>" + chiavi + r")(?![\w])", u, t)


def sigle(t, voci):
    for s in sorted(voci, key=len, reverse=True):
        v = voci[s] if isinstance(voci[s], dict) else {"testo": voci[s]}
        testo, genere = v["testo"], v.get("genere")
        sing, plur = v.get("singolare") or testo, v.get("plurale") or testo

        def sost(m, testo=testo, genere=genere, sing=sing, plur=plur):
            art = m.group("art")
            if not (art and genere):
                return (art + (" " if not art.endswith("'") else "") if art else "") + testo
            prep, num = _PREP[art.lower()]
            parola = plur if num == "p" else sing
            nuovo = articolo(prep, genere, num, parola)
            if art[:1].isupper():
                nuovo = nuovo[:1].upper() + nuovo[1:]
            return nuovo + parola
        t = re.sub(_ART + r"(?<![\w.])" + re.escape(s) + r"(?![\w])", sost, t)
    return t


def simboli(t):
    t = re.sub("[\U0001F000-\U0001FAFF☀-➿️‍]+", "", t)
    t = t.replace("`", "").replace("**", "").replace("__", "")
    t = re.sub(r"(?<!\w)[*_#]+(?=\w)|(?<=\w)[*_]+(?!\w)", "", t)
    t = re.sub(r"\s*&\s*", " e ", t)
    t = re.sub(r"\s*(?:→|⇒|->)\s*", ", poi ", t)
    t = re.sub(r"\s*[·•—|]\s*", ", ", t)
    t = re.sub(r"(?<=\d)\s*[x×]\s*(?=\d)", " per ", t)
    t = re.sub(r"\s*=\s*", " uguale ", t)
    t = re.sub(r"(?<=\d)\s*/\s*(?=\d)", " su ", t)
    t = re.sub(r"[\[\]{}<>^\\~]", " ", t)
    return t


def numeri(t):
    t = re.sub(r"(?<![\d.,])\d+(?:\.\d+){2,}(?!\d)",
               lambda m: m.group(0) if all(len(x) == 3 for x in m.group(0).split(".")[1:])
               else " punto ".join(intero(x) for x in m.group(0).split(".")), t)
    t = re.sub(r"(?<![\w.,\-])(\d{1,4})\s?[-–]\s?(\d{1,4})(?![\w\-]|[.,]\d)",
               lambda m: f"da {intero(m.group(1))} a {intero(m.group(2))}", t)
    t = re.sub(r"(?:(?<=^)|(?<=[\s(]))\+\s?(?=\d)", "più ", t)
    t = re.sub(r"(?:(?<=^)|(?<=[\s(]))[-−]\s?(?=\d)", "meno ", t)
    t = re.sub(r"(?<=[A-Za-zÀ-ÿ])-(?=\d)|(?<=\d)-(?=[A-Za-zÀ-ÿ])", " ", t)

    def n(m):
        i, sep, d = m.group("n"), m.group("sep"), m.group("d")
        if sep == "." and d and len(d) == 3:
            i, d = i + d, None
        elif sep == ".":
            return numero(i) + " punto " + decimali(d)
        p = numero(i, d)
        prima, dopo = m.string[max(0, m.start() - 1):m.start()], m.string[m.end():m.end() + 1]
        return (" " if prima[:1].isalpha() else "") + p + (" " if dopo[:1].isalpha() else "")
    return re.sub(r"(?<!\d)(?P<n>\d{1,3}(?:\.\d{3})+(?![.,]\d)|\d+)(?:(?P<sep>[.,])(?P<d>\d+))?(?!\d)", n, t)


def normalizza(t):
    if not t.strip():
        return ""
    d = dati()
    t = via_markdown(t)
    t = parlato(t, d["siti"])
    t = sostituzioni(t, d["frasi"])
    t = sostituzioni(t, {k: v for k, v in d["inglesi"].items() if v}, maiuscole=False)
    t = sostituzioni(t, d["nomi"], maiuscole=False)
    t = date(t)
    t = orari(t)
    t = importi(t)
    t = percentuali(t)
    t = ordinali(t)
    t = unita(t, d["unita"])
    t = sostituzioni(t, d["abbreviazioni"], maiuscole=False)
    t = sigle(t, d["sigle"])
    t = simboli(t)
    t = numeri(t)
    t = re.sub(r"\s+([,.;:!?])", r"\1", t)
    t = re.sub(r",\s*,+", ",", t)
    return re.sub(r"[ \t]{2,}", " ", t).strip()


if __name__ == "__main__":
    sys.stdout.write(normalizza(sys.stdin.read()) + "\n")
