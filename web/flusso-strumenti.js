// Lo stile del flusso degli strumenti (CONTRATTO-2J lavoro 2). «Completo»: ogni riga strumento come
// oggi. «Minimo»: le righe strumento consecutive (il ragionamento in mezzo non spezza il gruppo e resta
// visibile) diventano una riga compatta «3 strumenti · 2 s», che si apre con un tocco.
// Lavora sopra il DOM di chat.js: un osservatore sui figli di #messaggi rifà i gruppi a ogni riga
// nuova o riscritta; il giro è idempotente, quindi le sue modifiche non lo fanno ripartire all'infinito.

import { rigaDa } from './chat.js';
import { icona } from './icone.js';
import { bus, CHIAVI, impostazione } from './stato.js';
import { durata, el } from './util.js';

const aperti = new Set(); // id della prima riga dei gruppi aperti a mano
let cont = null;

export const flussoMinimo = () => impostazione(CHIAVI.flusso, 'completo') === 'minimo';

export function avviaFlussoStrumenti(contenitore) {
  if (cont) return;
  cont = contenitore;
  new MutationObserver(() => compatta()).observe(cont, { childList: true });
  bus.su('impostazioni', () => compatta());
  compatta();
}

/** «3 strumenti · 2 s», più «1 fallito» e «in corso» quando serve. */
export function riassuntoGruppo(righe) {
  const dati = righe.map((n) => rigaDa(n.dataset.id)).filter(Boolean);
  const c = (r) => r.corpo || {};
  const inCorso = dati.some((r) => c(r).stato === 'in_corso');
  const falliti = dati.filter((r) => c(r).stato !== 'in_corso' && !(c(r).ok === true || c(r).stato === 'riuscito')).length;
  const inizio = Math.min(...dati.map((r) => r.creata_ms || Infinity));
  const fine = Math.max(...dati.map((r) => r.aggiornata_ms || r.creata_ms || 0));
  const n = righe.length;
  return [`${n} ${n === 1 ? 'strumento' : 'strumenti'}`, Number.isFinite(inizio) && fine > inizio && !inCorso ? durata(fine - inizio) : null,
    falliti ? `${falliti} ${falliti === 1 ? 'fallito' : 'falliti'}` : null, inCorso ? 'in corso' : null].filter(Boolean).join(' · ');
}

function sommario(chiave) {
  const testo = el('span.testo');
  const tasto = el('button', { type: 'button', 'aria-expanded': 'false', onclick: () => {
    if (aperti.has(chiave)) aperti.delete(chiave); else aperti.add(chiave);
    compatta();
  } }, icona('strumento', 14), testo, icona('destra', 14, 'freccia'));
  return el('div.strumenti-compatti', { dati: { chiave } }, tasto);
}

function compatta() {
  if (!cont) return;
  const minimo = flussoMinimo();
  cont.classList.toggle('flusso-minimo', minimo);
  const vecchi = new Map([...cont.querySelectorAll(':scope > .strumenti-compatti')].map((n) => [n.dataset.chiave, n]));
  if (!minimo) {
    for (const n of vecchi.values()) n.remove();
    for (const n of cont.querySelectorAll(':scope > .riga-strumento.compattata')) n.classList.remove('compattata');
    return;
  }
  const gruppi = [];
  let g = null;
  for (const n of cont.children) {
    if (n.classList.contains('riga-strumento')) { if (!g) gruppi.push(g = []); g.push(n); } else if (!n.classList.contains('riga-ragionamento') && !n.classList.contains('strumenti-compatti')) g = null;
  }
  const usati = new Set();
  for (const righe of gruppi) {
    const chiave = righe[0].dataset.id;
    usati.add(chiave);
    const aperto = aperti.has(chiave);
    for (const n of righe) n.classList.toggle('compattata', !aperto);
    const s = vecchi.get(chiave) || sommario(chiave);
    if (righe[0].previousElementSibling !== s) righe[0].before(s);
    const testo = riassuntoGruppo(righe);
    const t = s.querySelector('.testo');
    if (t.textContent !== testo) t.textContent = testo;
    const b = s.querySelector('button');
    if (b.getAttribute('aria-expanded') !== String(aperto)) b.setAttribute('aria-expanded', String(aperto));
    b.title = aperto ? 'Richiudi gli strumenti' : 'Mostra gli strumenti';
  }
  for (const [k, n] of vecchi) if (!usati.has(k)) n.remove();
}
