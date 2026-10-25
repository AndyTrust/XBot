// Completamento di `@nome` nel campo (CONTRATTO-2D sez. 3): l'elenco degli assistenti con la loro
// `menzione` (nome senza spazi, minuscolo, unico, da GET /assistenti).

import { avatar } from './avatar.js';
import { stato } from './stato.js';
import { el } from './util.js';

export const menzioneDi = (a) => a.menzione || (a.nome || '').toLocaleLowerCase('it').replace(/\s+/g, '');

/** Aggancia il completamento a un campo di testo. Va chiamato prima degli altri keydown del campo. */
export function agganciaMenzioni(campo, contenitore) {
  const lista = el('div.menzioni', { role: 'listbox', 'aria-label': 'Assistenti da menzionare', hidden: true });
  contenitore.append(lista);
  let voci = [];
  let scelta = 0;
  let inizio = -1;

  const chiudi = () => { lista.hidden = true; voci = []; inizio = -1; campo.removeAttribute('aria-activedescendant'); };
  const segna = () => {
    lista.querySelectorAll('[role=option]').forEach((n, i) => n.setAttribute('aria-selected', i === scelta ? 'true' : 'false'));
    campo.setAttribute('aria-activedescendant', `menzione-${scelta}`);
  };
  const scegli = (i) => {
    const a = voci[i];
    if (!a) return;
    const fine = campo.selectionStart;
    const m = `@${menzioneDi(a)} `;
    campo.value = campo.value.slice(0, inizio) + m + campo.value.slice(fine);
    const pos = inizio + m.length;
    campo.setSelectionRange(pos, pos);
    chiudi();
    campo.dispatchEvent(new Event('input', { bubbles: true }));
    campo.focus();
  };

  campo.addEventListener('input', () => {
    const prima = campo.value.slice(0, campo.selectionStart);
    const m = /(^|\s)@([\p{L}\p{N}_.-]*)$/u.exec(prima);
    if (!m) { chiudi(); return; }
    inizio = prima.length - m[2].length - 1;
    const cerca = m[2].toLocaleLowerCase('it');
    voci = [...stato.assistenti.values()]
      .filter((a) => !a.archiviato_ms && a.id !== stato.assistenteId && (menzioneDi(a).includes(cerca) || (a.nome || '').toLocaleLowerCase('it').includes(cerca)))
      .slice(0, 8);
    if (!voci.length) { chiudi(); return; }
    scelta = 0;
    lista.replaceChildren(...voci.map((a, i) => el('button', {
      type: 'button', role: 'option', id: `menzione-${i}`, 'aria-selected': i === 0 ? 'true' : 'false',
      onpointerdown: (ev) => { ev.preventDefault(); scegli(i); },
    }, avatar(a, 22, { stato: false }), el('strong', {}, `@${menzioneDi(a)}`), el('span', {}, a.titolo || a.nome))));
    lista.hidden = false;
  });
  campo.addEventListener('keydown', (ev) => {
    if (lista.hidden) return;
    if (ev.key === 'ArrowDown' || ev.key === 'ArrowUp') {
      ev.preventDefault();
      scelta = (scelta + (ev.key === 'ArrowDown' ? 1 : -1) + voci.length) % voci.length;
      segna();
    } else if (ev.key === 'Enter' || ev.key === 'Tab') {
      ev.preventDefault();
      ev.stopImmediatePropagation();
      scegli(scelta);
    } else if (ev.key === 'Escape') {
      ev.preventDefault();
      ev.stopImmediatePropagation();
      chiudi();
    }
  });
  campo.addEventListener('blur', () => setTimeout(chiudi, 150));
}
