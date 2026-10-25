#!/usr/bin/env python3
"""Prepara l'albero pubblico di XBot in una cartella nuova, lo controlla e fa un commit unico.

    python3 ops/pubblica/prepara_pubblico.py <destinazione> --autore "Nome <email>"
        [--repo <radice del repository di lavoro>] [--rif HEAD] [--con-loghi]
        [--compila [--target-dir <cartella>]]

Passi:
 1. `git archive <rif>` del repository di lavoro, estratto nella destinazione (nuova o vuota);
 2. toglie i percorsi di `ops/pubblica/esclusi.txt` (i loghi di `web/loghi/` restano solo con
    `--con-loghi`);
 3. `AGENTS.md` e `README.md` diventano `ops/pubblica/AGENTS.pubblico.md` e
    `ops/pubblica/README.pubblico.md`, se ci sono;
 4. controlli bloccanti: chiavi e segreti riconoscibili, parole di `ops/pubblica/parole_vietate.txt`
    (con le eccezioni di `ops/pubblica/consentiti.txt`), file sopra 1 MB, file `.env` veri;
    ogni problema esce come `file:riga: motivo`, mai con il testo della riga;
 5. `gitleaks detect --no-git` se è installato;
 6. `git init` e un commit unico con l'autore dato: «XBot <data> da <hash di lavoro>»;
 7. con `--compila`, `cargo build --release` nella destinazione, con gli artefatti in
    `--target-dir` (predefinita `$CARGO_TARGET_DIR`, altrimenti `<destinazione>/target`).

Non fa push. Esce con 1 se un controllo fallisce (in quel caso non fa il commit).
Solo libreria standard.
"""

from __future__ import annotations

import argparse
import datetime as dt
import fnmatch
import io
import os
import re
import shutil
import subprocess
import sys
import tarfile
from pathlib import Path, PurePosixPath

MAX_BYTE = 1024 * 1024
CARTELLA_LOGHI = "web/loghi/"

# Schemi dei segreti: (nome, espressione). Mai stampare la corrispondenza.
SEGRETI = [
    ("chiave sk-", re.compile(rb"\bsk-[A-Za-z0-9_\-]{20,}")),
    ("chiave xai-", re.compile(rb"\bxai-[A-Za-z0-9_\-]{20,}")),
    ("token GitHub", re.compile(rb"\bgh[pousr]_[A-Za-z0-9]{30,}")),
    ("chiave Google AIza", re.compile(rb"\bAIza[0-9A-Za-z_\-]{30,}")),
    ("chiave AWS AKIA", re.compile(rb"\bAKIA[0-9A-Z]{16}\b")),
    ("token Google ya29.", re.compile(rb"\bya29\.[0-9A-Za-z_\-]{20,}")),
    ("token Slack", re.compile(rb"\bxox[bpa]-[0-9A-Za-z\-]{10,}")),
    ("chiave privata", re.compile(rb"-----BEGIN [A-Z ]*PRIVATE KEY-----")),
    ("seme otpauth://", re.compile(rb"otpauth://[a-z]+/[^\s\"'<>]*secret=[A-Z2-7]{16,}", re.I)),
]


def avviso(t: str) -> None:
    print(f"avviso: {t}", file=sys.stderr)


def git(radice: Path, *argomenti: str, **kw) -> subprocess.CompletedProcess:
    return subprocess.run(["git", "-C", str(radice), *argomenti], check=True, **kw)


def leggi_righe(file: Path) -> list[str]:
    """Le righe utili di un file di configurazione: senza commenti né righe vuote."""
    if not file.is_file():
        return []
    out = []
    for r in file.read_text(encoding="utf-8").splitlines():
        r = r.split("#", 1)[0].strip()
        if r:
            out.append(r)
    return out


def corrisponde(rel: str, schema: str) -> bool:
    """`rel` (posix, relativo) sta sotto lo schema di esclusi.txt? «x/» vale per la cartella."""
    cartella = schema.endswith("/")
    sp = PurePosixPath(schema.rstrip("/")).parts
    rp = PurePosixPath(rel).parts
    if len(rp) < len(sp) or not all(fnmatch.fnmatchcase(a, b) for a, b in zip(rp, sp)):
        return False
    return len(rp) > len(sp) if cartella else len(rp) == len(sp)


def estrai(radice: Path, rif: str, dest: Path) -> str:
    """`git archive` di `rif` nella destinazione; restituisce l'hash corto del commit."""
    hash_lavoro = git(radice, "rev-parse", "--short=12", rif, capture_output=True, text=True).stdout.strip()
    archivio = git(radice, "archive", "--format=tar", rif, capture_output=True).stdout
    with tarfile.open(fileobj=io.BytesIO(archivio)) as t:
        t.extractall(dest, filter="data")
    return hash_lavoro


def togli(dest: Path, schemi: list[str]) -> list[str]:
    tolti = []
    for p in sorted(dest.rglob("*"), key=lambda x: len(x.parts), reverse=True):
        if not (p.is_file() or p.is_symlink()):
            continue
        rel = p.relative_to(dest).as_posix()
        if any(corrisponde(rel, s) for s in schemi):
            p.unlink()
            tolti.append(rel)
    # cartelle rimaste vuote
    for p in sorted(dest.rglob("*"), key=lambda x: len(x.parts), reverse=True):
        if p.is_dir() and not any(p.iterdir()):
            p.rmdir()
    return tolti


def sostituisci(dest: Path) -> None:
    for pubblico, destinazione in [
        ("ops/pubblica/AGENTS.pubblico.md", "AGENTS.md"),
        ("ops/pubblica/README.pubblico.md", "README.md"),
    ]:
        s = dest / pubblico
        if s.is_file():
            shutil.move(str(s), str(dest / destinazione))
            print(f"{destinazione} ← {pubblico}")
        else:
            avviso(f"{pubblico} non c'è: {destinazione} resta quello del repository di lavoro")


def binario(dati: bytes) -> bool:
    return b"\0" in dati[:8192]


def numero_riga(dati: bytes, pos: int) -> int:
    return dati.count(b"\n", 0, pos) + 1


def env_vero(nome: str) -> bool:
    if nome.endswith((".esempio", ".example")):
        return False
    return nome == ".env" or nome.startswith(".env.") or nome.endswith(".env")


def controlla(dest: Path, parole: list[str], consentiti: set[tuple[str, str]]) -> list[str]:
    problemi = []
    schemi_parole = [(w, re.compile(re.escape(w.encode()), re.I)) for w in parole]
    for p in sorted(dest.rglob("*")):
        if p.is_symlink():
            problemi.append(f"{p.relative_to(dest).as_posix()}:0: collegamento simbolico")
            continue
        if not p.is_file():
            continue
        rel = p.relative_to(dest).as_posix()
        if rel.startswith(".git/"):
            continue
        dati = p.read_bytes()
        if len(dati) > MAX_BYTE:
            problemi.append(f"{rel}:0: file sopra 1 MB ({len(dati)} byte)")
        if env_vero(p.name):
            problemi.append(f"{rel}:0: file .env senza .esempio")
        for nome, rx in SEGRETI:
            for m in rx.finditer(dati):
                problemi.append(f"{rel}:{numero_riga(dati, m.start())}: possibile segreto ({nome})")
        for parola, rx in schemi_parole:
            if (rel, parola.lower()) in consentiti or rel == "ops/pubblica/consentiti.txt":
                continue
            if rx.search(rel.encode()):
                problemi.append(f"{rel}:0: parola vietata «{parola}» nel nome del file")
            righe = sorted({numero_riga(dati, m.start()) for m in rx.finditer(dati)})
            if righe:
                dove = "binario" if binario(dati) else "righe " + ",".join(map(str, righe[:20]))
                altre = f" (+{len(righe) - 20})" if len(righe) > 20 else ""
                problemi.append(f"{rel}:{righe[0]}: parola vietata «{parola}» ({dove}{altre})")
    return problemi


def gitleaks(dest: Path) -> bool:
    exe = shutil.which("gitleaks")
    if not exe:
        avviso("gitleaks non è installato: controllo saltato")
        return True
    r = subprocess.run(
        [exe, "detect", "--no-git", "--redact", "--no-banner", "-s", str(dest)],
        capture_output=True,
        text=True,
    )
    uscita = (r.stdout + r.stderr).strip().splitlines()
    print("gitleaks:", uscita[-1] if uscita else f"uscita {r.returncode}")
    return r.returncode == 0


def leggi_autore(testo: str) -> tuple[str, str]:
    m = re.fullmatch(r"\s*(.+?)\s*<([^<>\s]+@[^<>\s]+)>\s*", testo)
    if not m:
        raise SystemExit("--autore vuole «Nome <email>»")
    return m.group(1), m.group(2)


def committa(dest: Path, autore: tuple[str, str], messaggio: str) -> str:
    ambiente = {
        **os.environ,
        "GIT_AUTHOR_NAME": autore[0],
        "GIT_AUTHOR_EMAIL": autore[1],
        "GIT_COMMITTER_NAME": autore[0],
        "GIT_COMMITTER_EMAIL": autore[1],
        "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_CONFIG_NOSYSTEM": "1",
    }
    for k in ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"):
        ambiente.pop(k, None)
    c = ["git", "-C", str(dest), "-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false"]
    subprocess.run(c[:3] + ["init", "-q", "-b", "main"], check=True, env=ambiente)
    subprocess.run(c + ["add", "-A"], check=True, env=ambiente)
    subprocess.run(c + ["commit", "-q", "-m", messaggio], check=True, env=ambiente)
    return subprocess.run(
        c + ["rev-parse", "--short=12", "HEAD"], check=True, env=ambiente, capture_output=True, text=True
    ).stdout.strip()


def compila(dest: Path, target: Path) -> bool:
    ambiente = {**os.environ, "CARGO_TARGET_DIR": str(target)}
    print(f"cargo build --release (CARGO_TARGET_DIR={target})")
    r = subprocess.run(["cargo", "build", "--release"], cwd=dest, env=ambiente)
    print("compilazione:", "riuscita" if r.returncode == 0 else f"fallita ({r.returncode})")
    return r.returncode == 0


def main() -> int:
    a = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    a.add_argument("destinazione", type=Path)
    a.add_argument("--autore", required=True, help="«Nome <email>» del commit pubblico")
    a.add_argument("--repo", type=Path, help="radice del repository di lavoro (predefinita: quella di questo script)")
    a.add_argument("--rif", default="HEAD", help="commit da pubblicare (predefinito HEAD)")
    g = a.add_mutually_exclusive_group()
    g.add_argument("--con-loghi", action="store_true", help="tiene web/loghi/ (marchi di terzi)")
    g.add_argument("--senza-loghi", action="store_true", help="toglie web/loghi/ (il predefinito)")
    a.add_argument("--compila", action="store_true", help="cargo build --release nella destinazione")
    a.add_argument("--target-dir", type=Path, help="CARGO_TARGET_DIR della compilazione")
    o = a.parse_args()

    autore = leggi_autore(o.autore)
    partenza = o.repo or Path(__file__).resolve().parent
    radice = Path(
        git(partenza, "rev-parse", "--show-toplevel", capture_output=True, text=True).stdout.strip()
    )
    dest = o.destinazione.expanduser().resolve()
    if dest.exists() and (not dest.is_dir() or any(dest.iterdir())):
        print(f"errore: {dest} esiste e non è una cartella vuota", file=sys.stderr)
        return 2
    try:
        dest.relative_to(radice)
        print("errore: la destinazione non può stare dentro il repository di lavoro", file=sys.stderr)
        return 2
    except ValueError:
        pass
    dest.mkdir(parents=True, exist_ok=True)

    hash_lavoro = estrai(radice, o.rif, dest)
    print(f"Estratto {o.rif} ({hash_lavoro}) di {radice} in {dest}")

    conf = dest / "ops" / "pubblica"
    schemi = leggi_righe(conf / "esclusi.txt")
    if not schemi:
        print("errore: ops/pubblica/esclusi.txt manca o è vuoto nel commit", file=sys.stderr)
        return 2
    parole = leggi_righe(conf / "parole_vietate.txt")
    if not parole:
        # Il file può mancare nel commit: si prova quello su disco nel repository di lavoro.
        parole = leggi_righe(radice / "ops" / "pubblica" / "parole_vietate.txt")
    if not parole:
        avviso("parole_vietate.txt non c'è: controllo delle parole saltato (solo i segreti)")
    consentiti = set()
    for r in leggi_righe(conf / "consentiti.txt"):
        pezzi = r.split()
        if len(pezzi) >= 2:
            consentiti.add((pezzi[0], pezzi[1].lower()))
    if o.con_loghi:
        schemi = [s for s in schemi if s != CARTELLA_LOGHI]

    tolti = togli(dest, schemi)
    print(f"Tolti {len(tolti)} file secondo esclusi.txt")
    sostituisci(dest)

    problemi = controlla(dest, parole, consentiti)
    leaks_ok = gitleaks(dest)
    if problemi or not leaks_ok:
        print(f"\nCONTROLLI FALLITI: {len(problemi)} problemi" + ("" if leaks_ok else " + gitleaks"))
        for p in problemi:
            print("  " + p)
        print("Nessun commit fatto. La cartella resta per l'esame.")
        return 1
    print("Controlli: nessun segreto, nessuna parola vietata, nessun file sopra 1 MB.")

    data = dt.datetime.now().strftime("%Y-%m-%d %H:%M")
    h = committa(dest, autore, f"XBot {data} da {hash_lavoro}")
    print(f"Commit pubblico {h} in {dest} (autore {autore[0]}). Niente push.")
    target = (o.target_dir or Path(os.environ.get("CARGO_TARGET_DIR") or dest / "target")).expanduser()
    if o.compila and not compila(dest, target):
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
