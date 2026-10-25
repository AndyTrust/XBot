// Pezzi d'interfaccia comuni a pagine e fogli: righe, interruttori, tendine, campi, fogli.

import { icona } from './icone.js';
import { apriSovrapposta } from './menu.js';
import { copia, el } from './util.js';

export const coppia = (nome, controllo, nota) => el('div.coppia', {}, el('span', {}, nome, nota ? el('small', {}, nota) : null), controllo);

export function tendina(etichetta, opzioni, valore, f) {
  return el('select', { 'aria-label': etichetta, onchange: (ev) => f(ev.target.value) },
    opzioni.map(([v, t, disattivo]) => el('option', { value: v, selected: String(v) === String(valore ?? ''), disabled: disattivo }, t)));
}

export function interruttore(etichetta, acceso, f) {
  return el('input.interruttore', { type: 'checkbox', role: 'switch', 'aria-label': etichetta, checked: !!acceso, onchange: (ev) => f(ev.target.checked, ev.target) });
}

export const titolo = (t, ...dopo) => el('div.riquadro-titolo.con-tasti', {}, el('span', {}, t), ...dopo);
export const nota = (t) => el('p.nota', {}, t);
export const vuoto = (t) => el('p.nota.vuota', {}, t);

/** Un campo etichettato. `attr.rows` → textarea; `attr.tipo === 'select'` → opzioni. */
export function campo(etichetta, attr = {}, largo = false) {
  const { valore, opzioni, ...resto } = attr;
  let c;
  if (opzioni) c = el('select', resto, opzioni.map(([v, t]) => el('option', { value: v, selected: String(v) === String(valore ?? '') }, t)));
  else if (resto.rows) c = el('textarea', resto, valore ?? '');
  else c = el('input', { ...resto, value: valore ?? '' });
  return el(`label.campo-modulo${largo ? '.largo' : ''}`, {}, etichetta, c);
}

/** Un tasto piccolo con icona e testo. */
export const tastino = (testo, ic, onclick, extra = {}) => el('button.tasto.piccolo', { type: 'button', onclick, ...extra }, ic ? icona(ic, 14) : null, testo);

/** Un tasto solo icona con etichetta accessibile. */
export const tastoIcona = (ic, etichetta, onclick, extra = {}) => el('button.tasto-icona.piccolo', { type: 'button', 'aria-label': etichetta, title: etichetta, onclick, ...extra }, icona(ic, 15));

/**
 * Un foglio con testa, corpo (griglia di campi) e piede con Annulla / conferma.
 * `invia(modulo)` può lanciare: il messaggio va nella riga d'errore e il foglio resta aperto.
 */
export function foglio({ titolo: t, corpo, conferma = 'Salva', invia, piccolo = false, unaColonna = false, altriTasti = [] }) {
  const errore = el('p.errore-modulo', { role: 'alert', hidden: true });
  const tasto = el('button.tasto.primario', { type: 'submit' }, conferma);
  const modulo = el('form', { style: 'display:contents', onsubmit: async (ev) => {
    ev.preventDefault();
    tasto.disabled = true;
    errore.hidden = true;
    try {
      const resta = await invia?.(modulo);
      if (resta !== true) chiudi();
    } catch (e) {
      errore.textContent = e?.message || String(e);
      errore.hidden = false;
      errore.scrollIntoView({ block: 'nearest' });
    } finally { tasto.disabled = false; }
  } },
  el('div.foglio-testa', {}, el('h2', {}, t)),
  el(`div.foglio-corpo${unaColonna ? '.una' : ''}`, {}, ...[corpo].flat(), errore),
  el('div.foglio-piede', {}, ...altriTasti, el('span.spazio'), el('button.tasto', { type: 'button', onclick: () => chiudi() }, invia ? 'Annulla' : 'Chiudi'), invia ? tasto : null));
  const { chiudi, nodo } = apriSovrapposta(modulo, { classe: `foglio${piccolo ? '.piccolo' : ''}`, etichetta: t });
  setTimeout(() => nodo.querySelector('input:not([type=checkbox]),textarea,select')?.focus(), 0);
  return { chiudi, nodo, modulo };
}

/** Un valore da copiare: testo in mono e tasto Copia. */
export function daCopiare(valore, etichetta = 'Copia') {
  const codice = el('code', {}, valore);
  return el('div.da-copiare', {}, codice, el('button.tasto.piccolo', { type: 'button', onclick: () => copia(valore, 'Copiato', codice) }, icona('copia', 14), etichetta));
}

/** Legge i campi con nome di un modulo in un oggetto (le caselle diventano booleani). */
export function valoriDi(modulo) {
  const v = {};
  for (const c of modulo.querySelectorAll('[name]')) {
    if (c.type === 'checkbox') v[c.name] = c.checked;
    else v[c.name] = c.value;
  }
  return v;
}
