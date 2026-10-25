#!/usr/bin/env python3
"""Server MCP finto lento (Fase 2.L-bis): dorme LENTO_S secondi (predefinito 2) prima di
rispondere a initialize; uno strumento `caselle` in sola lettura che risponde «pec, lavoro, gmail»."""
import json, os, sys, time
for riga in sys.stdin:
    m = json.loads(riga)
    if "id" not in m:
        continue
    metodo, rid = m.get("method"), m["id"]
    if metodo == "initialize":
        time.sleep(float(os.environ.get("LENTO_S", "2")))
        r = {"protocolVersion": m["params"]["protocolVersion"], "capabilities": {"tools": {}},
             "serverInfo": {"name": "lento", "version": "1"}}
    elif metodo == "tools/list":
        r = {"tools": [{"name": "caselle", "description": "caselle (lento)", "inputSchema": {"type": "object"},
                        "annotations": {"readOnlyHint": True}}]}
    elif metodo == "tools/call":
        r = {"content": [{"type": "text", "text": "pec, lavoro, gmail"}]}
    else:
        r = {}
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": rid, "result": r}) + "\n"); sys.stdout.flush()
