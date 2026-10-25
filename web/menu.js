// Menu a comparsa e finestre sovrapposte: un solo menu aperto, Esc e clic fuori chiudono.

import { el } from './util.js';
import { icona } from './icone.js';

let aperto = null;

export function chiudiMenu() {
  if (!aperto) return;
  aperto.nodo.remove();
  document.removeEventListener('pointerdown', aperto.fuori, true);
  document.removeEventListener('keydown', aperto.tasti, true);
  const torna = aperto.origine;
  aperto = null;
  torna?.focus?.();
}

/**
 * Apre un menu vicino a `ancora` (un elemento o {x, y}).
 * voci: [{testo, icona, nota, azione, disattivo}] | '-' | {titolo}
 */
export function apriMenu(ancora, voci, { origine } = {}) {
  chiudiMenu();
  const nodo = el('div.menu', { role: 'menu' });
  for (const v of voci) {
    if (v === '-') { nodo.append(el('hr')); continue; }
    if (v.titolo) { nodo.append(el('div.menu-titolo', {}, v.titolo)); continue; }
    nodo.append(el('button', {
      role: 'menuitem', type: 'button', disabled: v.disattivo,
      onclick: () => { chiudiMenu(); v.azione?.(); },
    }, v.icona ? icona(v.icona, 16) : null, el('span', {}, v.testo), v.nota ? el('small', {}, v.nota) : null));
  }
  document.body.append(nodo);
  const r = ancora instanceof Element ? ancora.getBoundingClientRect() : { left: ancora.x, right: ancora.x, top: ancora.y, bottom: ancora.y };
  const m = nodo.getBoundingClientRect();
  let x = Math.min(r.left, innerWidth - m.width - 8);
  let y = r.bottom + 6;
  if (y + m.height > innerHeight - 8) y = Math.max(8, r.top - m.height - 6);
  nodo.style.left = `${Math.max(8, x)}px`;
  nodo.style.top = `${y}px`;
  const fuori = (ev) => { if (!nodo.contains(ev.target)) chiudiMenu(); };
  const tasti = (ev) => {
    const tasti = [...nodo.querySelectorAll('button:not(:disabled)')];
    const i = tasti.indexOf(document.activeElement);
    if (ev.key === 'Escape') { ev.preventDefault(); ev.stopPropagation(); chiudiMenu(); }
    else if (ev.key === 'ArrowDown') { ev.preventDefault(); tasti[(i + 1) % tasti.length]?.focus(); }
    else if (ev.key === 'ArrowUp') { ev.preventDefault(); tasti[(i - 1 + tasti.length) % tasti.length]?.focus(); }
  };
  document.addEventListener('pointerdown', fuori, true);
  document.addEventListener('keydown', tasti, true);
  aperto = { nodo, fuori, tasti, origine: origine || (ancora instanceof Element ? ancora : null) };
  nodo.querySelector('button:not(:disabled)')?.focus();
}

// ---------- Finestre sovrapposte ----------
const pila = [];

/** Apre una finestra sovrapposta. Restituisce {chiudi, nodo}. */
export function apriSovrapposta(contenuto, { classe = '', etichetta = '', inAlto = false, quandoChiusa } = {}) {
  // 2.L difetto 2: un menu aperto non resta sopra il foglio (anche quando il tocco non dà pointerdown)
  const origineMenu = aperto?.origine;
  chiudiMenu();
  const origine = origineMenu || document.activeElement;
  const modale = el(`div.modale${classe ? `.${classe}` : ''}`, { role: 'dialog', 'aria-modal': 'true', 'aria-label': etichetta });
  const sotto = el(`div.sotto${inAlto ? '.in-alto' : ''}`, {}, modale);
  const voce = { sotto, quandoChiusa, origine };
  const chiudi = () => {
    const i = pila.indexOf(voce);
    if (i < 0) return;
    pila.splice(i, 1);
    sotto.remove();
    quandoChiusa?.();
    origine?.focus?.();
  };
  voce.chiudi = chiudi;
  sotto.addEventListener('pointerdown', (ev) => { if (ev.target === sotto) chiudi(); });
  modale.append(el('button.tasto-icona.chiudi', { type: 'button', 'aria-label': 'Chiudi', onclick: chiudi }, icona('chiudi', 16)));
  modale.append(...[contenuto].flat());
  document.getElementById('sovrapposte').append(sotto);
  pila.push(voce);
  return { chiudi, nodo: modale };
}

export function sovrappostaAperta() { return pila.length > 0; }

document.addEventListener('keydown', (ev) => {
  if (ev.key === 'Escape' && pila.length && !aperto) {
    ev.preventDefault();
    pila[pila.length - 1].chiudi();
  }
});

// ---------- Piccoli fogli: un campo di testo, una conferma (al posto di prompt e confirm) ----------

/** Chiede un testo. Risolve con il testo pulito, o null se annullato. */
export function chiediTesto({ titolo, etichetta = 'Nome', valore = '', conferma = 'Salva', massimo = 80 }) {
  return new Promise((risolvi) => {
    let fatto = false;
    const campo = el('input', { value: valore, maxlength: massimo, autocomplete: 'off', required: true, 'aria-label': etichetta });
    const errore = el('p.errore-modulo', { role: 'alert', hidden: true });
    const modulo = el('form', { style: 'display:contents', onsubmit: (ev) => {
      ev.preventDefault();
      const t = campo.value.trim();
      if (!t) { errore.textContent = 'Serve un nome.'; errore.hidden = false; campo.focus(); return; }
      fatto = true; chiudi(); risolvi(t);
    } },
    el('div.foglio-testa', {}, el('h2', {}, titolo)),
    el('div.foglio-corpo', { style: 'grid-template-columns:1fr' }, el('label.campo-modulo.largo', {}, etichetta, campo), errore),
    el('div.foglio-piede', {}, el('button.tasto', { type: 'button', onclick: () => chiudi() }, 'Annulla'), el('button.tasto.primario', { type: 'submit' }, conferma)));
    const { chiudi } = apriSovrapposta(modulo, { classe: 'foglio.piccolo', etichetta: titolo, quandoChiusa: () => { if (!fatto) risolvi(null); } });
    campo.focus();
    campo.select();
  });
}

/** Chiede una conferma. Risolve con true o false. */
export function chiediConferma({ titolo, testo, conferma = 'Conferma', pericolo = false }) {
  return new Promise((risolvi) => {
    let esito = false;
    const si = el(`button.tasto.primario${pericolo ? '.pericolo-pieno' : ''}`, { type: 'button', onclick: () => { esito = true; chiudi(); } }, conferma);
    const { chiudi } = apriSovrapposta([
      el('div.foglio-testa', {}, el('h2', {}, titolo)),
      el('div.foglio-corpo', { style: 'grid-template-columns:1fr' }, el('p', { style: 'margin:0;color:var(--testo-2)' }, testo)),
      el('div.foglio-piede', {}, el('button.tasto', { type: 'button', onclick: () => chiudi() }, 'Annulla'), si),
    ], { classe: 'foglio.piccolo', etichetta: titolo, quandoChiusa: () => risolvi(esito) });
    si.focus();
  });
}
