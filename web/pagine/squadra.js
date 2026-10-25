// Squadra (CONTRATTO-2E sez. 4): da `GET /squadra` chi è ogni assistente, cosa fa, con cosa lavora,
// dove, le routine attive e l'ultimo turno; «Modifica» apre il foglio dell'assistente. Sotto, i
// conteggi dei modelli per macchina e provider («Mac · Cursor · 265 modelli») con «mostra tutti».
// A 1280 è una tabella; sotto 700 px ogni riga diventa una scheda (pagine.css).

import { api, elenco } from '../api.js';
import { apriFoglioAssistente } from '../assistente.js';
import { avatar } from '../avatar.js';
import { icona } from '../icone.js';
import { etichettaMacchina, etichettaModello as pulisciModello, leggiMacchine, macchinaDa } from '../macchine.js';
import { riepilogoModelli } from '../scelta.js';
import { assistenteDi, modelliDi, nomeProvider } from '../stato.js';
import { puntoComputer } from '../schermo.js';
import { nota, vuoto } from '../ui.js';
import { el, quandoBreve } from '../util.js';

/** Un campo della risposta che può essere un id, un nome o un oggetto {id, nome|etichetta}. */
const testo = (v) => (v && typeof v === 'object' ? v.etichetta || v.nome || v.id : v) || null;
const idDi = (v) => (v && typeof v === 'object' ? v.id : v) || null;

function conCosa(x) {
  const p = x.provider ? (typeof x.provider === 'object' ? testo(x.provider) : nomeProvider(x.provider)) : 'Auto';
  const m = testo(x.modello) && pulisciModello(testo(x.modello), { nome: p });
  return m ? `${p} · ${m}` : p;
}

/** Il modello arriva come id: l'etichetta («Grok 4.7 (Cursor)») si legge da `GET /modelli`. */
function etichettaModello(x, testoIniziale) {
  const n = el('span', {}, testoIniziale);
  const pid = idDi(x.provider);
  const mid = idDi(x.modello);
  if (pid && mid && typeof x.modello !== 'object') {
    modelliDi(pid).then((ms) => { const m = ms.find((y) => y.id === mid); if (m?.etichetta) n.textContent = `${nomeProvider(pid)} · ${pulisciModello(m, { nome: nomeProvider(pid) })}`; }).catch(() => {});
  }
  return n;
}

function dove(x) {
  if (!x.macchina) return 'Automatica';
  const m = macchinaDa(idDi(x.macchina));
  return m ? etichettaMacchina(m) : testo(x.macchina);
}

/** Come `squadra::quando` del server: cron, ogni N secondi, all'evento. */
function quandoRoutine(y) {
  let j = y.attivatore_json;
  if (typeof j === 'string') { try { j = JSON.parse(j); } catch { j = {}; } }
  j = j || {};
  if (y.attivatore_tipo === 'cron') return `cron ${j.expression || ''}`.trim();
  if (y.attivatore_tipo === 'ogni_secondi') return `ogni ${j.every_seconds || j.ogni_secondi || 0} secondi`;
  if (y.attivatore_tipo === 'evento') return `all'evento ${j.evento || ''}`.trim();
  return null;
}

function routine(x) {
  const r = x.routine_attive;
  if (Array.isArray(r)) return r.length ? r.map((y) => (typeof y === 'string' ? y : [y.nome, y.quando || quandoRoutine(y)].filter(Boolean).join(' · '))).join('; ') : 'nessuna';
  if (typeof r === 'number') return r ? `${r} attive` : 'nessuna';
  return 'nessuna';
}

export async function paginaSquadra() {
  const [r] = await Promise.all([api.squadra(), leggiMacchine()]);
  const conteggi = await riepilogoModelli().catch(() => []);
  const squadra = elenco(r, 'assistenti');
  if (!squadra.length) return [vuoto('Nessun assistente nella squadra.')];
  const cella = (etichetta, ...dentro) => el('td', { dati: { etichetta } }, ...dentro);
  const righe = squadra.map((x) => {
    const a = { ...assistenteDi(x.id), ...x };
    return el('tr', {},
      cella('Chi', el('span.chi-squadra', {}, avatar(a, 28, { stato: false }),
        el('span', {}, el('strong', {}, x.nome), el('small', {}, `@${x.menzione || (x.nome || '').toLowerCase()}`), x.titolo ? el('small', {}, x.titolo) : null))),
      cella('Cosa fa', x.descrizione || '—'),
      cella('Con cosa', etichettaModello(x, conCosa(x))),
      cella('Dove', dove(x), ' ', el('span.computer-squadra', {}, puntoComputer(x.computer?.stato || a.computer?.stato), el('small', {}, ` computer ${x.computer?.stato || a.computer?.stato || 'spento'}`))),
      cella('Routine', routine(x)),
      cella('Ultimo turno', x.ultimo_turno_ms ? quandoBreve(x.ultimo_turno_ms) : 'mai'),
      el('td.azione', {}, el('button.tasto.piccolo', {
        type: 'button', 'aria-label': `Modifica ${x.nome}`, onclick: () => apriFoglioAssistente(x.id, { scheda: 'modello' }), // al salvataggio la pagina si ridisegna da «assistente:aggiornato»
      }, icona('matita', 14), 'Modifica')));
  });
  return [
    nota('Ogni assistente riceve questa tabella nel suo testo di sistema: sa chi fa cosa, con quale modello e dove, e lo chiama con la @menzione.'),
    el('div.riquadro.tabella-contenitore', {}, el('table.tabella-squadra', {},
      el('thead', {}, el('tr', {}, ...['Chi', 'Cosa fa', 'Con cosa', 'Dove', 'Routine', 'Ultimo turno', ''].map((t) => el('th', { scope: 'col' }, t)))),
      el('tbody', {}, righe))),
    conteggi.length ? el('div.riquadro-titolo', {}, 'Modelli disponibili') : null,
    conteggi.length ? el('div.riquadro.conteggi-modelli', {}, conteggi) : null,
  ];
}
