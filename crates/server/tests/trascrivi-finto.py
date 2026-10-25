#!/usr/bin/env python3
"""Trascrizione finta per le prove: dice quanti byte ha il file e la lingua. Con la lingua
«rotto» risponde come lo script vero davanti a un file che non è audio."""
import json
import os
import sys

if sys.argv[2] == "rotto":
    print(json.dumps({"errore": "audio_non_valido"}))
    sys.exit(2)
print(json.dumps({"testo": f"ciao Mario ({os.path.getsize(sys.argv[1])} byte, {sys.argv[2]})",
                  "durata_ms": 3000, "modello": "finto"}))
