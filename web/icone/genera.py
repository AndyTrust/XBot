#!/usr/bin/env python3
"""Genera le icone della PWA da un'immagine quadrata del personaggio dell'app (Pillow):

    python3 web/icone/genera.py <immagine-512.png>

Escono app-192.png e app-512.png (personaggio al 88 % su sfondo pieno, per Android che non vuole
trasparenze) e app-maskable-512.png (al 66 %: resta dentro la zona sicura del ritaglio).
"""
import sys
from pathlib import Path

from PIL import Image

SFONDO = (244, 239, 230, 255)  # crema, si legge in chiaro e in scuro
QUI = Path(__file__).resolve().parent
if len(sys.argv) != 2:
    sys.exit("uso: genera.py <immagine-512.png>")
SORGENTE = Path(sys.argv[1]).expanduser()


def icona(misura, quota):
    fondo = Image.new("RGBA", (misura, misura), SFONDO)
    tipo = Image.open(SORGENTE).convert("RGBA")
    lato = int(misura * quota)
    tipo = tipo.resize((lato, lato), Image.LANCZOS)
    fondo.alpha_composite(tipo, ((misura - lato) // 2, (misura - lato) // 2))
    return fondo.convert("RGB")


if __name__ == "__main__":
    icona(192, 0.88).save(QUI / "app-192.png", optimize=True)
    icona(512, 0.88).save(QUI / "app-512.png", optimize=True)
    icona(512, 0.66).save(QUI / "app-maskable-512.png", optimize=True)
    print("app-192.png app-512.png app-maskable-512.png")
