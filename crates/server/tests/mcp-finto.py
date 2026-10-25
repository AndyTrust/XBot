#!/usr/bin/env python3
"""Un server MCP stdio finto per le prove dei collegamenti: initialize, tools/list, tools/call.
La descrizione di `cerca` dice le prime tre lettere di TOKEN_FINTO (l'ambiente decifrato)."""
import json
import os
import sys

for riga in sys.stdin:
    m = json.loads(riga)
    if "id" not in m:
        continue
    metodo, rid = m.get("method"), m["id"]
    if metodo == "initialize":
        r = {"protocolVersion": m["params"]["protocolVersion"], "capabilities": {"tools": {}},
             "serverInfo": {"name": "mcp-finto", "version": "1"}}
    elif metodo == "tools/list":
        token = os.environ.get("TOKEN_FINTO", "assente")[:3]
        r = {"tools": [
            {"name": "cerca", "description": f"cerca (token={token})", "inputSchema": {"type": "object"},
             "annotations": {"readOnlyHint": True}},
            {"name": "scrivi", "description": "scrive", "inputSchema": {"type": "object"}},
        ]}
    elif metodo == "tools/call":
        r = {"content": [{"type": "text", "text": "fatto"}]}
    else:
        r = {}
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": rid, "result": r}) + "\n")
    sys.stdout.flush()
