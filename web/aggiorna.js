// Aggiornamenti per chi installa (CONTRATTO-GITHUB sez. E): controllo all'avvio e ogni 6 ore con
// GET /aggiornamenti, avviso discreto nella barra (mai con modo «no»), «Aggiorna ora» con
// POST /aggiornamenti/avvia e attesa del riavvio, service worker con la cache legata a
// `versione_installata` di GET /salute (cache nuova a ogni versione, le vecchie le toglie `activate`).

import { chiama } from './api.js';
import { icona } from './icone.js';
import { avvisa, el } from './util.js';

const SEI_ORE = 6 * 3600 * 1000;
const OGNI_5_S = 5000;
const TRE_MINUTI = 3 * 60 * 1000;

/** Lo stato condiviso con la pagina Impostazioni → Aggiornamenti. */
export const agg = {
  dati: null, // l'ultima risposta di GET /aggiornamenti
  errore: null, // l'ultimo errore del controllo (rete, rotta assente…)
  installata: null, // `versione_installata` vista all'apertura della pagina
  installataNuova: null, // una versione diversa installata mentre la pagina era aperta
  inCorso: false,
};
let apriSezione = () => {};
let avviato = false;

/** La versione in forma di chiave per la cache (niente spazi né caratteri strani). */
const chiave = (v) => String(v || '').trim().replace(/[^\w.-]+/g, '_').slice(0, 60);

async function salute() {
  const r = await fetch('/api/v1/salute', { cache: 'no-store', credentials: 'same-origin' });
  if (!r.ok) throw new Error(`salute ${r.status}`);
  return r.json();
}

/** Registra il service worker con la versione nell'indirizzo: a versione nuova, worker e cache nuovi. */
export function registraSw(versione = agg.installataNuova || agg.installata) {
  if (!('serviceWorker' in navigator) || location.protocol === 'file:') return Promise.resolve(null);
  const k = chiave(versione);
  return navigator.serviceWorker.register(`/app/sw.js${k ? `?v=${encodeURIComponent(k)}` : ''}`, { scope: '/app/' }).catch(() => null);
}

/** Dopo un aggiornamento: worker nuovo e controllo subito, così nessun modulo vecchio resta in cache. */
async function rinnovaSw(versione) {
  const reg = await registraSw(versione);
  try { await reg?.update(); } catch { /* il prossimo avvio lo rifà */ }
  return reg;
}

/** Prima cosa all'apertura (senza accesso): legge la versione e registra il worker. */
export async function preparaVersione() {
  let s = null;
  try { s = await salute(); } catch { /* server giù: worker senza versione */ }
  agg.installata = s?.versione_installata || null;
  if (document.readyState === 'complete') registraSw();
  else addEventListener('load', () => registraSw(), { once: true });
  return s;
}

/** Un controllo: GET /aggiornamenti e, a parte, una versione cambiata sotto i piedi (modo «auto»). */
export async function controlla() {
  try {
    agg.dati = await chiama('GET', '/aggiornamenti');
    agg.errore = null;
  } catch (e) {
    agg.errore = e;
    if (e.stato === 404) agg.dati = null;
  }
  const ora = agg.dati?.versione_installata || (await salute().catch(() => null))?.versione_installata;
  if (ora && agg.installata && ora !== agg.installata && ora !== agg.installataNuova) {
    agg.installataNuova = ora;
    await rinnovaSw(ora);
  }
  disegnaAvviso();
  return agg;
}

/** Dopo l'accesso, una volta sola: controllo subito e poi ogni 6 ore. */
export function avviaControlli({ apri } = {}) {
  if (apri) apriSezione = apri;
  if (avviato) return;
  avviato = true;
  controlla();
  setInterval(controlla, SEI_ORE);
}

/** L'avviso in fondo alla barra: versione nuova da installare, oppure già installata e da ricaricare. */
export function disegnaAvviso() {
  let n = document.getElementById('avviso-aggiornamento');
  const d = agg.dati;
  const ricarica = Boolean(agg.installataNuova);
  const mostra = ricarica || (d?.disponibile === true && d.modo !== 'no');
  if (!mostra) { n?.remove(); return; }
  if (!n) {
    n = el('button.avviso-aggiornamento', { type: 'button', id: 'avviso-aggiornamento' });
    document.querySelector('#barra .barra-piede')?.before(n);
  }
  const testo = ricarica ? 'Versione nuova installata: ricarica' : `Aggiornamento disponibile: ${d.versione_remota || 'nuova versione'}`;
  n.replaceChildren(icona('aggiorna', 15), el('span', {}, testo));
  n.title = testo;
  n.onclick = ricarica ? () => location.reload() : () => apriSezione();
}

/** «Aggiorna ora»: POST, poi salute ogni 5 s per 3 minuti; ricarica quando la versione cambia. */
export async function aggiornaOra(aggiorna = () => {}) {
  if (agg.inCorso) return;
  const prima = agg.dati?.versione_installata || agg.installata;
  try {
    await chiama('POST', '/aggiornamenti/avvia');
  } catch (e) {
    if (e.stato === 403) avvisa('Aggiornamenti disattivati su questo server', 4000);
    else if (e.stato === 409) avvisa(/sorgente/i.test(`${e.codice} ${e.message}`) ? 'Sorgente sconosciuta: il server non sa da dove aggiornarsi' : 'Aggiornamento già in corso', 4000);
    else avvisa(`Aggiornamento non avviato: ${e.message || e.codice}`, 4000);
    return;
  }
  agg.inCorso = true;
  avvisa('Aggiornamento avviato: il server si riavvia tra poco', 4000);
  aggiorna();
  const fine = Date.now() + TRE_MINUTI;
  while (Date.now() < fine) {
    await new Promise((r) => setTimeout(r, OGNI_5_S));
    const v = (await salute().catch(() => null))?.versione_installata;
    if (v && v !== prima) {
      agg.installataNuova = v;
      avvisa(`Aggiornato a ${v.split(/\s+/)[0]}: ricarico`, 3000);
      await rinnovaSw(v);
      setTimeout(() => location.reload(), 800);
      return;
    }
  }
  agg.inCorso = false;
  avvisa('Il server non ha cambiato versione in 3 minuti: guarda il registro degli aggiornamenti sul server', 6000);
  aggiorna();
}
