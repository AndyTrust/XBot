// Cassaforte (Layout sez. 2.4, CONTRATTO-2D sez. 1): schede Tutti/Accessi/Carte/Contatti/Segreti,
// ricerca, «Nuovo elemento» con i quattro moduli e l'importazione CSV, rivela con l'occhio e copia,
// usi della voce, modifica ed eliminazione.

import { api, elenco, testoErrore } from '../api.js';
import { icona } from '../icone.js';
import { apriMenu, chiediConferma } from '../menu.js';
import { stato } from '../stato.js';
import { nota, tastoIcona, vuoto } from '../ui.js';
import { avvisa, copia, el, quandoBreve } from '../util.js';
import { foglioImporta, foglioVoce, NOMI_GENERE } from './cassaforte-moduli.js';

const SCHEDE = [['', 'Tutti'], ['accesso', 'Accessi'], ['carta', 'Carte'], ['contatto', 'Contatti'], ['segreto', 'Segreti']];
const ICONE = { accesso: 'lucchetto', carta: 'carta', contatto: 'persona', segreto: 'chiave' };
const NOMI_CAMPI = { password: 'Password', pin: 'PIN', passphrase: 'Passphrase', note: 'Note', numero: 'Numero', cvc: 'Codice', indirizzo: 'Indirizzo', valore: 'Valore' };
let scheda = '';
let cerca = '';

function sotto(v) {
  const r = v.riferimento || {};
  if (v.genere === 'accesso') return [r.utente, (v.siti || [])[0]?.replace(/^https?:\/\//, '')].filter(Boolean).join(' · ');
  if (v.genere === 'carta') return `•••• ${r.last4 || '????'}  ${r.exp_month ? `${String(r.exp_month).padStart(2, '0')}/${String(r.exp_year).slice(-2)}` : ''}${r.intestatario ? ` · ${r.intestatario}` : ''}`;
  if (v.genere === 'contatto') return [r.email, r.telefono, r.indirizzo?.citta].filter(Boolean).join(' · ');
  const chi = (v.assistenti || []).map((id) => stato.assistenti.get(id)?.nome).filter(Boolean);
  return [r.descrizione, chi.length ? `usato da: ${chi.join(', ')}` : 'nessun assistente'].filter(Boolean).join(' · ');
}

/** Il riquadro dei campi rivelati: mascherati, con occhio e copia per ciascuno. */
function campiRivelati(campi) {
  const voci = Object.entries(campi || {}).filter(([, x]) => x !== null && x !== undefined && x !== '');
  if (!voci.length) return vuoto('Nessun campo cifrato.');
  return el('div.rivelati', {}, voci.map(([k, x]) => {
    const testo = typeof x === 'object' ? JSON.stringify(x) : String(x);
    const valore = el('code', {}, '••••••••');
    let visibile = false;
    const occhio = tastoIcona('occhio', `Mostra ${NOMI_CAMPI[k] || k}`, () => {
      visibile = !visibile;
      valore.textContent = visibile ? testo : '••••••••';
      occhio.replaceChildren(icona(visibile ? 'occhiochiuso' : 'occhio', 15));
    });
    return el('div.riga-elenco', {}, el('span.riga-testi', {}, el('small', {}, NOMI_CAMPI[k] || k), valore), occhio,
      tastoIcona('copia', `Copia ${NOMI_CAMPI[k] || k}`, () => copia(testo, `${NOMI_CAMPI[k] || k} copiato`)));
  }));
}

function riga(v, ridisegna) {
  const extra = el('div.voce-extra', { hidden: true });
  const mostra = async (tipo) => {
    if (!extra.hidden && extra.dataset.tipo === tipo) { extra.hidden = true; extra.replaceChildren(); return; }
    extra.dataset.tipo = tipo;
    extra.hidden = false;
    extra.replaceChildren(el('p.nota', {}, 'Carico…'));
    try {
      if (tipo === 'rivela') extra.replaceChildren(campiRivelati((await api.rivela(v.id))?.campi));
      else {
        const usi = elenco(await api.usi(v.id), 'usi');
        extra.replaceChildren(usi.length ? el('div.elenco', {}, usi.slice(0, 12).map((u) => el('div.riga-elenco', {},
          el('span.riga-testi', {}, el('span', {}, `${u.assistente || stato.io?.nome || 'Tu'} · ${u.azione || 'uso'}`), el('small', {}, quandoBreve(u.ms)))))) : vuoto('Mai usata.'));
      }
    } catch (e) { extra.replaceChildren(vuoto(testoErrore(e))); }
  };
  return el('div.voce-cassaforte', {},
    el('div.riga-elenco', {},
      el('span.icona-tonda', {}, icona(ICONE[v.genere] || 'chiave', 16)),
      el('span.riga-testi', {}, el('strong', {}, v.titolo), el('small', {}, sotto(v) || NOMI_GENERE[v.genere])),
      v.genere !== 'contatto' ? tastoIcona('occhio', `Rivela «${v.titolo}»`, () => mostra('rivela')) : null,
      tastoIcona('sessioni', `Usi di «${v.titolo}»`, () => mostra('usi')),
      tastoIcona('matita', `Modifica «${v.titolo}»`, () => foglioVoce(v.genere, v, {}, ridisegna)),
      tastoIcona('cestino', `Elimina «${v.titolo}»`, async () => {
        if (!await chiediConferma({ titolo: `Eliminare «${v.titolo}»?`, testo: 'La voce sparisce dalla cassaforte e gli assistenti non la possono più usare.', conferma: 'Elimina', pericolo: true })) return;
        try { await api.togliVoce(v.id); avvisa('Eliminata'); ridisegna(); } catch (e) { avvisa(testoErrore(e)); }
      })),
    extra);
}

export async function paginaCassaforte({ ridisegna, opzioni = {} }) {
  if (opzioni.nuovo) setTimeout(() => foglioVoce(opzioni.nuovo, null, { assistenti: opzioni.assistenti }, ridisegna), 0);
  const tutte = elenco(await api.cassaforte(null, cerca), 'voci');
  const conta = (g) => tutte.filter((v) => !g || v.genere === g).length;
  const voci = tutte.filter((v) => !scheda || v.genere === scheda);
  const campoCerca = el('input', { type: 'search', placeholder: 'Cerca', value: cerca, 'aria-label': 'Cerca nella cassaforte' });
  let timer;
  campoCerca.addEventListener('input', () => { clearTimeout(timer); timer = setTimeout(() => { cerca = campoCerca.value.trim(); ridisegna(); }, 300); });
  const nuovo = el('button.tasto.primario', { type: 'button', 'aria-haspopup': 'menu', onclick: (ev) => apriMenu(ev.currentTarget, [
    ...Object.entries(NOMI_GENERE).map(([g, n]) => ({ testo: n, icona: ICONE[g], azione: () => foglioVoce(g, null, {}, ridisegna) })),
    '-',
    { testo: 'Importa da CSV', icona: 'carica', azione: () => foglioImporta(ridisegna) },
  ]) }, icona('piu', 14), 'Nuovo elemento');
  if (cerca) setTimeout(() => { const c = document.querySelector('.cerca-cassaforte input'); if (c) { c.focus(); c.setSelectionRange(c.value.length, c.value.length); } }, 0);
  return [
    el('div.riga-tasti.testa-pagina', {}, el('div.schede-pillola', { role: 'tablist' }, SCHEDE.map(([g, n]) => el('button', {
      type: 'button', role: 'tab', 'aria-selected': scheda === g ? 'true' : 'false', onclick: () => { scheda = g; ridisegna(); },
    }, `${n} ${conta(g)}`))), el('span.spazio'), nuovo),
    el('label.cerca-cassaforte', {}, icona('cerca', 16), campoCerca),
    el('div.riquadro.elenco', {}, voci.length ? voci.map((v) => riga(v, ridisegna)) : vuoto(cerca ? 'Nessuna voce trovata.' : 'La cassaforte è vuota: aggiungi un elemento o importa un CSV.')),
    nota('Il bot riceve numero e codice della carta solo dopo il tuo sì in chat, una volta per uso. Vedono accessi e contatti per inserirli al posto tuo, e i segreti solo come variabili d\'ambiente, dopo il tuo sì.'),
  ];
}
