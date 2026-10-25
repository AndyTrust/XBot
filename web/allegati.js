// Allegati della barra di scrittura: caricamento subito (POST /sessioni/{id}/allegati → {id, nome,
// blob_hash}), anteprima e rimozione prima dell'invio; poi `allegati[]` nell'invio.
// Si allega con ＋, incollando un'immagine o trascinando file sulla chat.

import { api, testoErrore } from './api.js';
import { icona } from './icone.js';
import { ricordaAllegato } from './chat.js';
import { stato } from './stato.js';
import { el, dimensione } from './util.js';

const MASSIMO = 25 * 1024 * 1024;
let voci = []; // {chiave, file, stato: 'carico'|'pronto'|'errore', allegato, anteprima, errore}
let contenitore;
let quandoCambia = () => {};

export function preparaAllegati(nodo, cambio) {
  contenitore = nodo;
  quandoCambia = cambio;
  const chat = document.getElementById('chat');
  chat.addEventListener('dragover', (ev) => { if (ev.dataTransfer?.types?.includes('Files')) { ev.preventDefault(); chat.classList.add('trascina'); } });
  chat.addEventListener('dragleave', (ev) => { if (ev.target === chat || !chat.contains(ev.relatedTarget)) chat.classList.remove('trascina'); });
  chat.addEventListener('drop', (ev) => {
    chat.classList.remove('trascina');
    if (!ev.dataTransfer?.files?.length) return;
    ev.preventDefault();
    aggiungi(ev.dataTransfer.files);
  });
}

/** Carica subito i file scelti. */
export function aggiungi(files) {
  if (!stato.sessione) return;
  for (const f of files) {
    const v = { chiave: crypto.randomUUID?.() || String(Math.random()), file: f, stato: 'carico', sessione: stato.sessione.id };
    if (f.size > MASSIMO) { v.stato = 'errore'; v.errore = 'oltre 25 MB'; }
    if (/^image\//.test(f.type)) v.anteprima = URL.createObjectURL(f);
    voci.push(v);
    if (v.stato === 'carico') carica(v);
  }
  disegna();
}

async function carica(v) {
  try {
    const a = await api.allega(v.sessione, v.file);
    v.allegato = { id: a.id, nome: a.nome || v.file.name, blob_hash: a.blob_hash, tipo_mime: v.file.type, dimensione: v.file.size };
    v.stato = 'pronto';
    ricordaAllegato({ ...v.allegato, ...a });
  } catch (e) {
    v.stato = 'errore';
    v.errore = testoErrore(e);
  }
  disegna();
}

function togli(v) {
  voci = voci.filter((x) => x !== v);
  if (v.anteprima) URL.revokeObjectURL(v.anteprima);
  disegna();
}

function disegna() {
  if (!contenitore) return;
  const mie = voci.filter((v) => v.sessione === stato.sessione?.id);
  contenitore.hidden = !mie.length;
  contenitore.replaceChildren(...mie.map((v) => el(`div.chip-allegato.${v.stato}`, { title: v.errore || v.file.name },
    v.anteprima ? el('img', { src: v.anteprima, alt: '' }) : icona('file', 18),
    el('span.chip-testi', {}, el('span', {}, v.file.name), el('small', {}, v.stato === 'carico' ? 'carico…' : v.stato === 'errore' ? v.errore : dimensione(v.file.size))),
    el('button', { type: 'button', 'aria-label': `Togli ${v.file.name}`, onclick: () => togli(v) }, icona('chiudi', 12)))));
  quandoCambia();
}

/** Gli allegati pronti della sessione aperta (per l'invio). */
export const pronti = () => voci.filter((v) => v.sessione === stato.sessione?.id && v.stato === 'pronto').map((v) => v.allegato);
export const inCaricamento = () => voci.some((v) => v.sessione === stato.sessione?.id && v.stato === 'carico');

/** Dopo l'invio: via gli allegati della sessione. */
export function svuotaAllegati() {
  for (const v of voci.filter((x) => x.sessione === stato.sessione?.id)) if (v.anteprima) setTimeout(() => URL.revokeObjectURL(v.anteprima), 60000);
  voci = voci.filter((x) => x.sessione !== stato.sessione?.id);
  disegna();
}

export const ridisegnaAllegati = disegna;
