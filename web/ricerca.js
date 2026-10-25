// Ricerca e tavolozza (⌘K, Layout sez. 4): assistenti, sessioni, azioni («Nuovo assistente»,
// «Impostazioni»). La ricerca nei messaggi aspetta FTS5 sul server.

import { api } from './api.js';
import { avatarSvg } from './avatar.js';
import { icona } from './icone.js';
import { apriSovrapposta } from './menu.js';
import { stato } from './stato.js';
import { el, quandoBreve } from './util.js';

const norm = (s) => (s || '').toLocaleLowerCase('it').normalize('NFD').replace(/[̀-ͯ]/g, '');

let sessioniCache = null; // [{sessione, assistente}]

async function tutteLeSessioni() {
  if (sessioniCache) return sessioniCache;
  const elenchi = await Promise.all([...stato.assistenti.values()].map(async (a) => {
    try { return (await api.sessioni(a.id)).map((s) => ({ s, a })); } catch { return []; }
  }));
  sessioniCache = elenchi.flat().sort((x, y) => y.s.aggiornata_ms - x.s.aggiornata_ms);
  setTimeout(() => { sessioniCache = null; }, 60000);
  return sessioniCache;
}

/** azioni: {apri(assistenteId, sessioneId?), nuovo(nome), impostazioni()} */
export function apriRicerca(azioni) {
  const campo = el('input', { type: 'text', placeholder: 'Cerca assistenti e sessioni…', 'aria-label': 'Cerca', autocomplete: 'off', spellcheck: 'false' });
  const esiti = el('div.tavolozza-esiti', { role: 'listbox', 'aria-label': 'Risultati' });
  let voci = [];
  let scelta = 0;
  let sessioni = [];

  const disegna = () => {
    const q = norm(campo.value.trim());
    const assistenti = [...stato.assistenti.values()].filter((a) => !q || norm(`${a.nome} ${a.titolo} ${a.descrizione}`).includes(q)).slice(0, 8);
    const sess = sessioni.filter(({ s, a }) => !q || norm(`${s.titolo || ''} ${a.nome}`).includes(q)).slice(0, 8);
    voci = [];
    const gruppi = [];
    const gruppo = (titolo, elenco) => {
      if (!elenco.length) return;
      gruppi.push(el('div.tavolozza-gruppo', {}, titolo));
      for (const v of elenco) {
        const i = voci.push(v) - 1;
        gruppi.push(el('button.tavolozza-voce', {
          type: 'button', role: 'option', id: `voce-${i}`, 'aria-selected': i === scelta ? 'true' : 'false',
          onclick: () => { chiudi(); v.azione(); }, onmousemove: () => { if (scelta !== i) { scelta = i; segna(); } },
        }, v.icona, el('span', {}, v.testo), v.nota ? el('small', {}, v.nota) : null));
      }
    };
    gruppo('Assistenti', assistenti.map((a) => ({
      testo: a.titolo ? `${a.nome} — ${a.titolo}` : a.nome, icona: avatarSvg(a, 22), azione: () => azioni.apri(a.id),
    })));
    gruppo('Sessioni', sess.map(({ s, a }) => ({
      testo: s.titolo || `Sessione con ${a.nome}`, nota: `${a.nome} · ${quandoBreve(s.aggiornata_ms)}`, icona: icona('sessioni', 18), azione: () => azioni.apri(a.id, s.id),
    })));
    gruppo('Azioni', [
      { testo: q ? `Nuovo assistente «${campo.value.trim()}»` : 'Nuovo assistente', icona: icona('piu', 18), azione: () => azioni.nuovo(campo.value.trim()) },
      { testo: 'Impostazioni', icona: icona('impostazioni', 18), azione: () => azioni.impostazioni() },
    ].filter((v) => !q || norm(v.testo).includes(q) || v.testo.startsWith('Nuovo')));
    if (scelta >= voci.length) scelta = Math.max(0, voci.length - 1);
    esiti.replaceChildren(...(gruppi.length ? gruppi : [el('div.tavolozza-nulla', {}, 'Nessun risultato')]));
    segna();
  };
  const segna = () => {
    esiti.querySelectorAll('.tavolozza-voce').forEach((n, i) => n.setAttribute('aria-selected', i === scelta ? 'true' : 'false'));
    campo.setAttribute('aria-activedescendant', `voce-${scelta}`);
    esiti.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' });
  };
  campo.addEventListener('input', () => { scelta = 0; disegna(); });
  campo.addEventListener('keydown', (ev) => {
    if (ev.key === 'ArrowDown') { ev.preventDefault(); scelta = (scelta + 1) % Math.max(1, voci.length); segna(); }
    else if (ev.key === 'ArrowUp') { ev.preventDefault(); scelta = (scelta - 1 + voci.length) % Math.max(1, voci.length); segna(); }
    else if (ev.key === 'Enter') { ev.preventDefault(); const v = voci[scelta]; if (v) { chiudi(); v.azione(); } }
  });
  const { chiudi } = apriSovrapposta([el('div.tavolozza-campo', {}, icona('cerca', 18), campo), esiti], { classe: 'tavolozza', etichetta: 'Cerca', inAlto: true });
  disegna();
  campo.focus();
  tutteLeSessioni().then((v) => { sessioni = v; disegna(); }).catch(() => {});
}
