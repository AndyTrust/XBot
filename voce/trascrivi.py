#!/usr/bin/env python3
"""Trascrizione per XBot (Fase 2.D): `trascrivi.py <file audio> [lingua]`.

Usa faster-whisper sulla CPU in int8 (audio decodificato con ffmpeg); il modello è `XBOT_VOCE_MODELLO` (predefinito «small»)
e si scarica al primo uso nella cache di Hugging Face (~/.cache). Stampa una riga JSON:
{"testo": "...", "durata_ms": 1234, "modello": "small"} oppure {"errore": "..."} con uscita
1 (2 e {"errore": "audio_non_valido"} se il file non si decodifica).
"""
import json
import os
import shutil
import subprocess
import sys


def audio_16k(file):
    """L'audio a 16 kHz mono come array float32, decodificato da ffmpeg (il decoder PyAV di
    faster-whisper non va con av 19). Senza ffmpeg si passa il file com'è."""
    if not shutil.which("ffmpeg"):
        return file
    import numpy as np
    r = subprocess.run(["ffmpeg", "-nostdin", "-loglevel", "error", "-i", file, "-f", "s16le",
                        "-ac", "1", "-ar", "16000", "-"], capture_output=True, timeout=60, check=True)
    return np.frombuffer(r.stdout, dtype=np.int16).astype(np.float32) / 32768.0


def main():
    if len(sys.argv) < 2:
        print(json.dumps({"errore": "uso: trascrivi.py <file> [lingua]"}))
        return 1
    file = sys.argv[1]
    lingua = (sys.argv[2] if len(sys.argv) > 2 else "it") or None
    modello = os.environ.get("XBOT_VOCE_MODELLO") or "small"
    try:
        from faster_whisper import WhisperModel
    except ImportError:
        print(json.dumps({"errore": "faster-whisper non installato nel python della voce"}))
        return 1
    try:
        audio = audio_16k(file)
        if not isinstance(audio, str) and len(audio) == 0:
            raise ValueError("vuoto")
    except Exception:
        print(json.dumps({"errore": "audio_non_valido"}))
        return 2
    try:
        m = WhisperModel(modello, device="cpu", compute_type="int8")
        pezzi, info = m.transcribe(audio, language=lingua, vad_filter=True, beam_size=5)
        testo = " ".join(p.text.strip() for p in pezzi).strip()
    except Exception as e:  # il server riceve il motivo, mai l'audio
        print(json.dumps({"errore": f"{type(e).__name__}: {e}"[:300]}))
        return 1
    print(json.dumps({"testo": testo, "durata_ms": int(round(info.duration * 1000)), "modello": modello},
                     ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main())
