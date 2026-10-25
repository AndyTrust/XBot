#!/usr/bin/env python3
"""Server di autorizzazione OAuth e server MCP finti sulla stessa porta (prove della 2.I).

Uso: oauth-finto.py <porta>

- /.well-known/oauth-protected-resource[/mcp]   metadati della risorsa (RFC 9728)
- /.well-known/oauth-authorization-server[...]  metadati del server (RFC 8414)
- POST /register     registrazione dinamica (client pubblico)
- GET  /authorize    risponde subito con il reindirizzamento a redirect_uri?code&state
- POST /token        codice (con PKCE S256) → token; refresh_token → token nuovo
- POST /revoke       revoca (RFC 7009)
- GET  /userinfo     {"email": "utente@finto.it"} con un Bearer valido
- POST /mcp          Streamable HTTP: 401 senza Bearer valido, `initialize`, `tools/list` (eco)
- POST /muto         (2.K) 403 senza sfida, ma con i metadati della risorsa `/muto`
- POST /controllo    {"durata": s, "rifiuta_refresh": bool, "scadi": bool} per le prove
- GET  /stato        contatori: registrazioni, scambi, rinnovi, revoche

Solo libreria standard. Nessun segreto vero: token «at-N» e «rt-N».
"""
import base64
import hashlib
import json
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlencode, urlparse

PORTA = int(sys.argv[1]) if len(sys.argv) > 1 else 8831
BASE = f"http://127.0.0.1:{PORTA}"
LOCK = threading.Lock()
S = {
    "durata": 3600, "rifiuta_refresh": False, "n": 0,
    "codici": {}, "accessi": {}, "refresh": {},
    "registrazioni": 0, "scambi": 0, "rinnovi": 0, "revoche": 0,
}


def nuovo(prefisso):
    S["n"] += 1
    return f"{prefisso}-{S['n']}"


def emetti(client_id):
    at, rt = nuovo("at"), nuovo("rt")
    S["accessi"][at] = time.time() + S["durata"]
    S["refresh"][rt] = client_id
    return {"access_token": at, "token_type": "Bearer", "expires_in": S["durata"], "refresh_token": rt}


class Gestore(BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def manda(self, codice, corpo=None, intestazioni=None):
        dati = b"" if corpo is None else json.dumps(corpo).encode()
        self.send_response(codice)
        for k, v in (intestazioni or {}).items():
            self.send_header(k, v)
        if corpo is not None:
            self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(dati)))
        self.end_headers()
        self.wfile.write(dati)

    def corpo(self):
        n = int(self.headers.get("Content-Length") or 0)
        return self.rfile.read(n) if n else b""

    def bearer_valido(self):
        a = self.headers.get("Authorization") or ""
        if not a.lower().startswith("bearer "):
            return False
        scade = S["accessi"].get(a[7:].strip())
        return scade is not None and scade > time.time()

    def metadati(self):
        return {
            "issuer": BASE,
            "authorization_endpoint": BASE + "/authorize",
            "token_endpoint": BASE + "/token",
            "registration_endpoint": BASE + "/register",
            "revocation_endpoint": BASE + "/revoke",
            "userinfo_endpoint": BASE + "/userinfo",
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code", "refresh_token"],
            "code_challenge_methods_supported": ["S256"],
            "token_endpoint_auth_methods_supported": ["none"],
        }

    def do_GET(self):
        u = urlparse(self.path)
        q = {k: v[0] for k, v in parse_qs(u.query).items()}
        with LOCK:
            if u.path.startswith("/.well-known/oauth-protected-resource/muto"):
                return self.manda(200, {"resource": BASE + "/muto", "authorization_servers": [BASE]})
            if u.path.startswith("/.well-known/oauth-protected-resource"):
                return self.manda(200, {"resource": BASE + "/mcp", "authorization_servers": [BASE]})
            if u.path.startswith("/.well-known/oauth-authorization-server") or u.path.startswith(
                    "/.well-known/openid-configuration"):
                return self.manda(200, self.metadati())
            if u.path == "/authorize":
                if q.get("code_challenge_method") != "S256" or not q.get("code_challenge"):
                    return self.manda(400, {"error": "invalid_request"})
                codice = nuovo("codice")
                S["codici"][codice] = (q.get("code_challenge"), q.get("client_id"))
                dove = q["redirect_uri"] + "?" + urlencode({"code": codice, "state": q.get("state", "")})
                return self.manda(302, None, {"Location": dove})
            if u.path == "/userinfo":
                if not self.bearer_valido():
                    return self.manda(401, {"error": "invalid_token"})
                return self.manda(200, {"email": "utente@finto.it", "sub": "1"})
            if u.path == "/stato":
                return self.manda(200, {k: S[k] for k in ("registrazioni", "scambi", "rinnovi", "revoche")})
            if u.path == "/mcp":
                return self.manda(405, {"error": "solo POST"})
        self.manda(404, {"error": "non trovato"})

    def do_DELETE(self):
        self.manda(200, {})

    def do_POST(self):
        u = urlparse(self.path)
        grezzo = self.corpo()
        with LOCK:
            if u.path == "/register":
                d = json.loads(grezzo or b"{}")
                S["registrazioni"] += 1
                return self.manda(201, {"client_id": nuovo("cid"), "redirect_uris": d.get("redirect_uris", []),
                                        "client_name": d.get("client_name")})
            if u.path == "/token":
                f = {k: v[0] for k, v in parse_qs(grezzo.decode()).items()}
                if f.get("grant_type") == "authorization_code":
                    sfida = S["codici"].pop(f.get("code"), None)
                    if not sfida:
                        return self.manda(400, {"error": "invalid_grant"})
                    v = f.get("code_verifier", "")
                    calcolata = base64.urlsafe_b64encode(hashlib.sha256(v.encode()).digest()).rstrip(b"=").decode()
                    if calcolata != sfida[0]:
                        return self.manda(400, {"error": "invalid_grant", "error_description": "pkce"})
                    S["scambi"] += 1
                    return self.manda(200, emetti(sfida[1]))
                if f.get("grant_type") == "refresh_token":
                    rt = f.get("refresh_token")
                    if S["rifiuta_refresh"] or rt not in S["refresh"]:
                        return self.manda(400, {"error": "invalid_grant"})
                    S["rinnovi"] += 1
                    cid = S["refresh"].pop(rt)
                    return self.manda(200, emetti(cid))
                return self.manda(400, {"error": "unsupported_grant_type"})
            if u.path == "/revoke":
                f = {k: v[0] for k, v in parse_qs(grezzo.decode()).items()}
                S["refresh"].pop(f.get("token"), None)
                S["accessi"].pop(f.get("token"), None)
                S["revoche"] += 1
                return self.manda(200, {})
            if u.path == "/controllo":
                d = json.loads(grezzo or b"{}")
                if "durata" in d:
                    S["durata"] = d["durata"]
                if "rifiuta_refresh" in d:
                    S["rifiuta_refresh"] = d["rifiuta_refresh"]
                if d.get("scadi"):
                    S["accessi"].clear()
                return self.manda(200, {"ok": True})
            if u.path == "/mcp":
                if not self.bearer_valido():
                    meta = BASE + "/.well-known/oauth-protected-resource/mcp"
                    return self.manda(401, {"error": "invalid_token"},
                                      {"WWW-Authenticate": f'Bearer resource_metadata="{meta}"'})
                return self.mcp(json.loads(grezzo or b"{}"))
            if u.path == "/muto":
                return self.manda(403, {"error": "vietato"})
        self.manda(404, {"error": "non trovato"})

    def mcp(self, m):
        metodo, mid = m.get("method"), m.get("id")
        if mid is None:
            return self.manda(202)
        if metodo == "initialize":
            ris = {"protocolVersion": m.get("params", {}).get("protocolVersion", "2025-06-18"),
                   "capabilities": {"tools": {}}, "serverInfo": {"name": "finto-oauth", "version": "1"}}
        elif metodo == "tools/list":
            ris = {"tools": [{"name": "eco", "description": "ripete il testo",
                              "inputSchema": {"type": "object", "properties": {"testo": {"type": "string"}}},
                              "annotations": {"readOnlyHint": True}}]}
        elif metodo == "tools/call":
            t = m.get("params", {}).get("arguments", {}).get("testo", "")
            ris = {"content": [{"type": "text", "text": t}]}
        else:
            return self.manda(200, {"jsonrpc": "2.0", "id": mid, "error": {"code": -32601, "message": "no"}})
        self.manda(200, {"jsonrpc": "2.0", "id": mid, "result": ris})


if __name__ == "__main__":
    ThreadingHTTPServer(("127.0.0.1", PORTA), Gestore).serve_forever()
