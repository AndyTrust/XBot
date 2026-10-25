#!/usr/bin/env python3
"""Imita sul Mac il blocco Caddy di `ops/google-mcp/caddy-google-mcp.txt` (Fase 2.K), porta 8860.

  /.well-known/oauth-protected-resource/google-mcp*   -> container, percorso intero
  /.well-known/oauth-authorization-server/google-mcp* -> container, /.well-known/oauth-authorization-server
  /google-mcp/*                                       -> container, senza /google-mcp
  il resto                                            -> XBot (127.0.0.1:8841)

Il container ascolta su 127.0.0.1:18851. Solo libreria standard; per la prova a mano
`google_ospitato_a_mano` di `s2k_oauth.rs`.
"""
import http.client
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

CONTAINER, XBOT = ("127.0.0.1", 18851), ("127.0.0.1", 8841)


def dove(p):
    if p.startswith("/.well-known/oauth-protected-resource/google-mcp"):
        return CONTAINER, p
    if p.startswith("/.well-known/oauth-authorization-server/google-mcp"):
        return CONTAINER, "/.well-known/oauth-authorization-server"
    if p.startswith("/google-mcp/"):
        return CONTAINER, p[len("/google-mcp"):]
    return XBOT, p


class Gestore(BaseHTTPRequestHandler):
    def log_message(self, *a):
        sys.stderr.write(f"caddy-finto {self.command} {self.path}\n")

    def tutto(self):
        (host, porta), p = dove(self.path)
        n = int(self.headers.get("Content-Length") or 0)
        corpo = self.rfile.read(n) if n else None
        c = http.client.HTTPConnection(host, porta, timeout=60)
        intest = {k: v for k, v in self.headers.items() if k.lower() != "host"}
        intest["Host"] = intest["X-Forwarded-Host"] = "127.0.0.1:8860"
        intest["X-Forwarded-Proto"] = "http"
        c.request(self.command, p, body=corpo, headers=intest)
        r = c.getresponse()
        dati = r.read()
        self.send_response(r.status)
        for k, v in r.getheaders():
            if k.lower() not in ("transfer-encoding", "content-length", "connection"):
                self.send_header(k, v)
        self.send_header("Content-Length", str(len(dati)))
        self.end_headers()
        self.wfile.write(dati)

    do_GET = do_POST = do_DELETE = do_PUT = do_PATCH = tutto


ThreadingHTTPServer(("127.0.0.1", 8860), Gestore).serve_forever()
