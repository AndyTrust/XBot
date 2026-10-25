#!/usr/bin/env python3
"""Il desktop finto delle prove della Fase 2.H: fa da Xvfb, gestore di finestre, x11vnc, Chrome e
ttyd. Il primo argomento dice il ruolo; gli altri sono quelli veri del programma che imita.

- xvfb: crea `$FINTO_X11/X<N>` (il socket che aspetta il server) e resta vivo;
- wm: resta vivo;
- x11vnc: ascolta su `-rfbport`, manda «RFB 003.008\\n» e poi fa eco;
- chrome: ascolta su `--remote-debugging-port=` e risponde in HTTP;
- ttyd: ascolta su `-p`, risponde in HTTP con il percorso chiesto (sotto `-b`).

Con `FINTO_MUORI_<RUOLO>=<secondi>` il ruolo esce da solo dopo quei secondi (codice 3).
"""
import os
import signal
import socket
import sys
import threading
import time

ruolo = sys.argv[1]
argomenti = sys.argv[2:]
fermo = threading.Event()


def valore(nome):
    for i, a in enumerate(argomenti):
        if a == nome and i + 1 < len(argomenti):
            return argomenti[i + 1]
        if a.startswith(nome + "="):
            return a.split("=", 1)[1]
    return None


def esci(*_):
    pulisci()
    os._exit(0)


def pulisci():
    if ruolo == "xvfb":
        try:
            os.remove(socket_x)
        except OSError:
            pass


signal.signal(signal.SIGTERM, esci)
signal.signal(signal.SIGINT, esci)


def ascolta(porta, servi):
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(("127.0.0.1", int(porta)))
    s.listen(16)

    def giro():
        while True:
            c, _ = s.accept()
            threading.Thread(target=servi, args=(c,), daemon=True).start()

    threading.Thread(target=giro, daemon=True).start()


def vnc(c):
    try:
        c.sendall(b"RFB 003.008\n")
        while True:
            d = c.recv(4096)
            if not d:
                break
            c.sendall(d)
    except OSError:
        pass
    finally:
        c.close()


def leggi_esatti(c, n):
    b = b""
    while len(b) < n:
        d = c.recv(n - len(b))
        if not d:
            raise OSError("chiuso")
        b += d
    return b


def websocket(c, richiesta):
    """Handshake con protocollo `tty`, poi eco dei frame di testo con «eco:» davanti."""
    import base64, hashlib
    chiave = ""
    for riga in richiesta.split("\r\n"):
        if riga.lower().startswith("sec-websocket-key:"):
            chiave = riga.split(":", 1)[1].strip()
    accetta = base64.b64encode(
        hashlib.sha1((chiave + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest()
    ).decode()
    c.sendall(
        ("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
         f"Sec-WebSocket-Accept: {accetta}\r\nSec-WebSocket-Protocol: tty\r\n\r\n").encode()
    )
    while True:
        h = leggi_esatti(c, 2)
        op, n = h[0] & 0x0F, h[1] & 0x7F
        if n == 126:
            n = int.from_bytes(leggi_esatti(c, 2), "big")
        elif n == 127:
            n = int.from_bytes(leggi_esatti(c, 8), "big")
        maschera = leggi_esatti(c, 4) if h[1] & 0x80 else b"\0\0\0\0"
        dati = bytes(b ^ maschera[i % 4] for i, b in enumerate(leggi_esatti(c, n)))
        if op == 8:
            c.sendall(b"\x88\x00")
            return
        if op in (1, 2):
            r = b"eco:" + dati
            c.sendall(bytes([0x80 | op, len(r)]) + r)


def http(etichetta):
    def servi(c):
        try:
            richiesta = c.recv(65536).decode("latin-1")
            if "upgrade: websocket" in richiesta.lower():
                websocket(c, richiesta)
                return
            prima = richiesta.split("\r\n", 1)[0]
            parti = prima.split(" ")
            percorso = parti[1] if len(parti) > 1 else "/"
            corpo = f"{etichetta} {parti[0]} {percorso}".encode()
            c.sendall(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n"
                + f"Content-Length: {len(corpo)}\r\nConnection: close\r\n\r\n".encode()
                + corpo
            )
        except OSError:
            pass
        finally:
            c.close()

    return servi


socket_x = None
if ruolo == "xvfb":
    display = next(a for a in argomenti if a.startswith(":"))[1:]
    socket_x = os.path.join(os.environ["FINTO_X11"], "X" + display)
    open(socket_x, "w").close()
elif ruolo == "x11vnc":
    ascolta(valore("-rfbport"), vnc)
elif ruolo == "chrome":
    ascolta(valore("--remote-debugging-port"), http("chrome finto"))
elif ruolo == "ttyd":
    ascolta(valore("-p"), http("ttyd finto"))
elif ruolo == "wm":
    pass
else:
    sys.exit(f"ruolo sconosciuto: {ruolo}")

muori = os.environ.get("FINTO_MUORI_" + ruolo.upper())
inizio = time.time()
while True:
    time.sleep(0.05)
    if muori and time.time() - inizio > float(muori):
        pulisci()
        os._exit(3)
