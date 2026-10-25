# Avvisi di terzi

XBot include nel repository una sola raccolta di codice di terzi come sorgente: noVNC, con la sua copia di pako, in `web/vendor/novnc/`. Le dipendenze Rust non stanno nel repository: Cargo le scarica da crates.io al momento della compilazione. I font non sono inclusi: l'interfaccia usa i font di sistema.

## Codice incluso

| Componente | Versione | Licenza | Percorso | Obblighi rispettati |
|---|---|---|---|---|
| noVNC (client VNC in HTML5), The noVNC Authors | 1.3.0 | MPL-2.0 | `web/vendor/novnc/core/` (tranne i due file sotto) | Le intestazioni di copyright e licenza restano in ogni file. L'avviso di licenza di noVNC 1.3.0 è `web/vendor/novnc/LICENSE.noVNC-1.3.0.txt`, il testo completo della MPL-2.0 è `web/vendor/novnc/LICENSE.MPL-2.0`. I file restano sotto MPL-2.0 e il loro sorgente è in questo repository. Una modifica a un file di noVNC va segnata in questa tabella e resta MPL-2.0. |
| `core/base64.js` (Digital Creations 2, Joel Martin) | come noVNC | MPL-2.0 | `web/vendor/novnc/core/base64.js` | Come noVNC. |
| `core/des.js` (Widget Workshop, Jef Poskanzer, AT&T Laboratories Cambridge) | come noVNC | BSD-2-Clause | `web/vendor/novnc/core/des.js` | L'avviso di copyright e il testo della licenza restano nel file e in `LICENSE.txt`. |
| pako, versione ES6 ridotta (Vitaly Puzrin) | derivata da pako 1.0.3 | MIT (Expat) | `web/vendor/novnc/vendor/pako/lib/utils/` e `vendor/pako/README.md` | L'avviso di copyright e il testo MIT sono in `vendor/pako/LICENSE` (quello di noVNC 1.3.0) e in `LICENSE.txt` (sezione `Expat`). |
| zlib di pako (imaya) | come pako | Zlib | `web/vendor/novnc/vendor/pako/lib/zlib/` | L'avviso resta in `LICENSE.txt` (sezione `Zlib`); i file non sono presentati come originali di XBot. |

### Confronto con noVNC 1.3.0

Confronto file per file del 2026-10-25 con il rilascio ufficiale `v1.3.0` (https://github.com/novnc/noVNC/archive/refs/tags/v1.3.0.tar.gz, SHA-256 `ee8f91514c9ce9f4054d132f5f97167ee87d9faa6630379267e569d789290336`, `package.json` versione 1.3.0):

- Identici a 1.3.0, byte per byte: tutti i 31 file di `core/` (anche `core/decoders/`, `core/input/`, `core/util/`) e i 13 file di `vendor/pako/` (`README.md`, `lib/utils/common.js`, `lib/zlib/*.js`). Nessun file di codice è modificato e nessuno viene da un'altra versione.
- Aggiunti da XBot, e non codice: `LICENSE.MPL-2.0` (testo da mozilla.org; il `docs/LICENSE.MPL-2.0` di 1.3.0 ha lo stesso testo, con `http://` al posto di `https://` nell'Exhibit A e uno spazio in fondo a una riga), `LICENSE.noVNC-1.3.0.txt` (il `LICENSE.txt` di 1.3.0, copiato così com'è) e `vendor/pako/LICENSE` (quello di 1.3.0, copiato così com'è).
- Diverso da 1.3.0: `LICENSE.txt`. Non è il `LICENSE.txt` di noVNC ma il file di copyright nel formato Debian (quello del pacchetto `novnc` delle distribuzioni). Le licenze che dichiara sono le stesse di 1.3.0 e contiene i testi completi di BSD-2-Clause, OFL-1.1, CC-BY-SA-3.0, Zlib ed Expat. Cita anche l'Orbitron, le pagine HTML, i CSS e le immagini di `app/`: quelle parti di noVNC non sono incluse in XBot. La riga «See /usr/share/common-licenses/MPL-2.0» rimanda a un percorso dei sistemi Debian: qui il testo è in `LICENSE.MPL-2.0` (scaricato da https://www.mozilla.org/media/MPL/2.0/index.txt, 373 righe, SHA-256 `3f3d9e0024b1921b067d6f7f88deb4a60cbe7a78e76c64e3f1d7fc3b779b9d04`).

## Dipendenze Rust (non incluse)

`Cargo.lock` elenca 423 pacchetti, tutti da crates.io. Le licenze dichiarate sono permissive: MIT, Apache-2.0 (anche con LLVM-exception), le loro combinazioni, ISC, BSD-2-Clause, BSD-3-Clause, Zlib, Unicode-3.0, Unlicense, CC0-1.0, 0BSD, BSL-1.0 e CDLA-Permissive-2.0. Due pacchetti offrono anche LGPL-2.1-or-later come alternativa a MIT o Apache-2.0: si usa MIT o Apache-2.0. Chi distribuisce i binari compilati deve allegare gli avvisi delle dipendenze, per esempio con `cargo about` o `cargo deny`.

Il conteggio è uscito da `cargo metadata --locked` il 2026-10-25.

## Opere dell'autore del progetto

Le illustrazioni dei personaggi in `web/avatar/personaggi/` sono dell'autore del progetto e sono rilasciate con la stessa licenza del codice (MIT, vedi `LICENSE`; decisione dell'autore del 2026-10-25).

## Icone degli avatar (`web/avatar/*.svg`)

Confronto del 2026-10-25 dei tracciati (`d`) delle 34 icone con due set di icone a licenza Apache-2.0: Material Design Icons di Pictogrammers (pacchetto npm `@mdi/svg` 7.4.47, 7447 icone) e Material Icons di Google (pacchetto npm `@material-design-icons/svg` 0.14.15, tutti gli stili).

- Nessuna icona è identica a un'icona dei due set.
- `aereo.svg` è un ridisegno di «flight» di Material Icons (Google, Apache-2.0): stessa forma e stesse misure, spostata di 1 unità verso l'alto e scritta con archi al posto delle curve. Si tratta come opera derivata: licenza Apache-2.0, avviso «Basata sull'icona "flight" di Material Icons, Copyright Google LLC, Apache License 2.0», testo della licenza su https://www.apache.org/licenses/LICENSE-2.0.
- Le altre 33 (`ancora`, `bilancia`, `busta`, `casa`, `chiave`, `coniglio`, `corona`, `cuore`, `diamante`, `fiamma`, `foglia`, `fotocamera`, `fulmine`, `gatto`, `globo`, `grafico`, `gufo`, `ingranaggio`, `lampadina`, `libro`, `luna`, `nota`, `orso`, `pesce`, `razzo`, `robot`, `scudo`, `sole`, `stella`, `tartaruga`, `tazza`, `uccello`, `volpe`) non somigliano a nessuna icona dei due set: sono tracciati semplici, entrati nel commit dell'interfaccia 2.D (2026-10-24) senza fonte. Sono opere del progetto, con la stessa licenza del codice (MIT, vedi `LICENSE`).

## Da verificare

- I loghi dei servizi in `web/loghi/` sono marchi dei rispettivi proprietari e non fanno parte dell'albero pubblico (`ops/pubblica/esclusi.txt`).
