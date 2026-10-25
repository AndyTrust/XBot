// Routine di un assistente (CONTRATTO-2D sez. 2): elenco con interruttori, «＋», modifica,
// «Esegui ora», elimina. Lo usano il pannello Dettagli e le impostazioni dell'assistente.

import { api, elenco, testoErrore } from '../api.js';
import { chiediConferma } from '../menu.js';
import { campo, foglio, interruttore, tastoIcona, titolo, vuoto } from '../ui.js';
import { bus } from '../stato.js';
import { avvisa, el, quandoBreve } from '../util.js';

const EVENTI = [['posta_nuova', 'posta nuova'], ['file_cambiato', 'file cambiato'], ['avvio', 'avvio del server']];

function json(v) {
  if (typeof v === 'string') { try { return JSON.parse(v); } catch { return {}; } }
  return v || {};
}

// Forma del contratto (sez. 12): cron {"expression"}, ogni_secondi {"every_seconds"}; si leggono anche cron/secondi.
const cronDi = (a) => a.expression || a.cron || '';
const secondiDi = (a) => Number(a.every_seconds || a.secondi || a.ogni_secondi || 0);

/** «ogni giorno alle 20:15», «ogni 2 ore», «cron 0 7 * * 1», «a evento: posta nuova». */
export function quando(r) {
  const a = json(r.attivatore_json);
  if (r.attivatore_tipo === 'cron') {
    const m = /^(\d{1,2}) (\d{1,2}) \* \* \*$/.exec(cronDi(a));
    return m ? `ogni giorno alle ${m[2]}:${m[1].padStart(2, '0')}` : `cron ${cronDi(a) || '?'}`;
  }
  if (r.attivatore_tipo === 'ogni_secondi') {
    const s = secondiDi(a);
    if (s < 60 || s % 60) return `ogni ${s} s`;
    if (s < 3600 || s % 3600) return `ogni ${s / 60} min`;
    return `ogni ${s / 3600} ${s === 3600 ? 'ora' : 'ore'}`;
  }
  return `a evento: ${EVENTI.find(([k]) => k === a.evento)?.[1] || a.evento || '?'}`;
}

/** Il foglio per creare o modificare una routine. */
export function foglioRoutine(assistenteId, r = null, dopo = () => {}) {
  const a = json(r?.attivatore_json);
  const giorno = /^(\d{1,2}) (\d{1,2}) \* \* \*$/.exec(cronDi(a));
  const modo = !r ? 'giorno' : r.attivatore_tipo === 'cron' ? (giorno ? 'giorno' : 'cron') : r.attivatore_tipo === 'ogni_secondi' ? 'ore' : 'evento';
  const sceltaModo = campo('Quando', { name: 'modo', valore: modo, opzioni: [['giorno', 'Ogni giorno alle…'], ['ore', 'Ogni N secondi, minuti o ore'], ['cron', 'Espressione cron'], ['evento', 'A un evento']] });
  const ora = campo('Ora', { name: 'ora', type: 'time', valore: giorno ? `${giorno[2].padStart(2, '0')}:${giorno[1].padStart(2, '0')}` : '07:00' });
  const sec = secondiDi(a);
  const unita = !sec ? 3600 : sec % 3600 === 0 ? 3600 : sec % 60 === 0 ? 60 : 1;
  const ore = el('div.campo-modulo', {}, 'Ogni',
    el('span.con-valore', {}, el('input', { name: 'ore', type: 'number', min: 1, max: 100000, value: sec ? sec / unita : 6, 'aria-label': 'Quanti' }),
      el('select', { name: 'unita', 'aria-label': 'Unità' }, [[1, 'secondi'], [60, 'minuti'], [3600, 'ore']].map(([v, t]) => el('option', { value: v, selected: v === unita }, t)))));
  const cron = campo('Cron (min ora giorno mese settimana)', { name: 'cron', valore: cronDi(a) || '0 7 * * 1-5', placeholder: '0 7 * * 1-5' });
  const evento = campo('Evento', { name: 'evento', valore: a.evento || 'posta_nuova', opzioni: EVENTI });
  const mostra = () => {
    const m = sceltaModo.querySelector('select').value;
    ora.hidden = m !== 'giorno'; ore.hidden = m !== 'ore'; cron.hidden = m !== 'cron'; evento.hidden = m !== 'evento';
  };
  sceltaModo.querySelector('select').addEventListener('change', mostra);
  const attiva = el('input', { type: 'checkbox', name: 'abilitata', checked: r ? !!r.abilitata : true });
  foglio({
    titolo: r ? `Routine «${r.nome}»` : 'Nuova routine',
    conferma: r ? 'Salva' : 'Crea',
    corpo: [
      campo('Nome', { name: 'nome', valore: r?.nome || '', required: true, maxlength: 80 }, true),
      campo('Cosa deve fare', { name: 'corpo', valore: r?.corpo || '', rows: 4, required: true, placeholder: 'es. Fai il giro delle chiusure e scrivimi il riassunto' }, true),
      sceltaModo, ora, ore, cron, evento,
      el('label.casella.largo', {}, attiva, 'Attiva'),
    ],
    invia: async (m) => {
      const v = Object.fromEntries(new FormData(m));
      let tipo; let att;
      if (v.modo === 'giorno') { const [h, mi] = (v.ora || '07:00').split(':'); tipo = 'cron'; att = { expression: `${Number(mi)} ${Number(h)} * * *` }; }
      else if (v.modo === 'ore') {
        const secondi = Math.round(Number(v.ore || 0) * Number(v.unita || 3600));
        if (!(secondi >= 5)) throw new Error('L\'intervallo minimo è di 5 secondi.');
        tipo = 'ogni_secondi'; att = { every_seconds: secondi };
      }
      else if (v.modo === 'cron') { tipo = 'cron'; att = { expression: (v.cron || '').trim() }; if (att.expression.split(/\s+/).length !== 5) throw new Error('Il cron vuole cinque campi.'); }
      else { tipo = 'evento'; att = { evento: v.evento }; }
      const dati = { nome: v.nome.trim(), corpo: v.corpo.trim(), attivatore_tipo: tipo, attivatore_json: att, abilitata: attiva.checked };
      try {
        if (r) await api.modificaRoutine(r.id, dati); else await api.creaRoutine(assistenteId, dati);
      } catch (e) { throw new Error(testoErrore(e)); }
      avvisa(r ? 'Routine salvata' : 'Routine creata');
      dopo();
      bus.emetti('assistente:aggiornato', assistenteId);
    },
  });
  mostra();
}

/** L'elenco delle routine con titolo e «＋». Restituisce il nodo, che si riempie da solo. */
export function sezioneRoutine(assistenteId, { compatta = false } = {}) {
  const lista = el('div.riquadro.elenco');
  const nodo = el('div.sezione-pannello', {},
    titolo('Routine', tastoIcona('piu', 'Nuova routine', () => foglioRoutine(assistenteId, null, carica))), lista);
  async function carica() {
    lista.replaceChildren(el('p.nota', {}, 'Carico…'));
    let elencoR = [];
    try { elencoR = elenco(await api.routine(assistenteId), 'routine'); } catch (e) { lista.replaceChildren(vuoto(testoErrore(e))); return; }
    if (!elencoR.length) { lista.replaceChildren(vuoto('Nessuna routine. Crea la prima con ＋.')); return; }
    lista.replaceChildren(...elencoR.map((r) => el('div.riga-elenco', {},
      el('span.riga-testi', {}, el('strong', {}, r.nome), el('small', {}, quando(r) + (r.prossima_ms ? ` · prossima ${quandoBreve(r.prossima_ms)}` : r.ultima_esecuzione_ms ? ` · ultima ${quandoBreve(r.ultima_esecuzione_ms)}` : ''))),
      tastoIcona('play', `Esegui ora «${r.nome}»`, async () => {
        try { await api.eseguiRoutine(r.id); avvisa(`«${r.nome}» avviata`); } catch (e) { avvisa(testoErrore(e)); }
      }),
      tastoIcona('matita', `Modifica «${r.nome}»`, () => foglioRoutine(assistenteId, r, carica)),
      compatta ? null : tastoIcona('cestino', `Elimina «${r.nome}»`, async () => {
        if (!await chiediConferma({ titolo: `Eliminare «${r.nome}»?`, testo: 'La routine non partirà più.', conferma: 'Elimina', pericolo: true })) return;
        try { await api.togliRoutine(r.id); carica(); bus.emetti('assistente:aggiornato', assistenteId); } catch (e) { avvisa(testoErrore(e)); }
      }),
      interruttore(`Routine «${r.nome}» attiva`, r.abilitata, async (si, nodoI) => {
        try { await api.modificaRoutine(r.id, { abilitata: si }); avvisa(si ? 'Accesa' : 'Spenta'); } catch (e) { nodoI.checked = !si; avvisa(testoErrore(e)); }
      }))));
  }
  carica();
  return nodo;
}
