// Utilizzo (Layout sez. 2.7, CONTRATTO-2D sez. 8): anelli delle quote e tabelle da GET /utilizzo,
// con i filtri periodo, provider e assistente. Niente fatturazione: sono gli abbonamenti.
// 2.L: i provider spenti (Codex) non compaiono fra le quote né nel filtro; sotto i 420 px la riga va in colonna.

import { api } from '../api.js';
import { anelloGrande } from '../impostazioni.js';
import { caricaQuote, nomeProvider, providerAcceso, providerAccesi, quotaDi, stato } from '../stato.js';
import { nota, tendina, vuoto } from '../ui.js';
import { barreQuota, quotaScaduta } from './quota-barre.js';
import { compatto, el, numero, quandoBreve } from '../util.js';

const GIORNO = 86400000;
let filtri = { giorni: 7, provider: '', assistente: '' };

const costo = (v) => (v ? `${numero(v, 2)} $` : '—');
const tot = (r) => Number(r.token ?? (Number(r.token_ingresso || 0) + Number(r.token_uscita || 0)));

function tabella(titoli, righe) {
  return el('div.riquadro.scorre', {}, el('table.tabella', {},
    el('thead', {}, el('tr', {}, titoli.map((t, i) => el(i ? 'th.num' : 'th', {}, t)))),
    el('tbody', {}, righe.map((r) => el('tr', {}, r.map((c, i) => el(i ? 'td.num' : 'td', {}, c)))))));
}

/** Barre dei giorni: un piccolo grafico a colonne in SVG. */
function barreGiorni(giorni) {
  if (!giorni.length) return null;
  const max = Math.max(...giorni.map(tot), 1);
  const w = 28;
  const ns = 'http://www.w3.org/2000/svg';
  const s = document.createElementNS(ns, 'svg');
  s.setAttribute('viewBox', `0 0 ${giorni.length * w} 90`);
  s.setAttribute('class', 'barre');
  s.setAttribute('role', 'img');
  s.setAttribute('aria-label', 'Token per giorno');
  s.innerHTML = giorni.map((g, i) => {
    const h = Math.max(2, (tot(g) / max) * 70);
    return `<rect x="${i * w + 5}" y="${74 - h}" width="${w - 10}" height="${h}" rx="3"><title>${g.giorno}: ${compatto(tot(g))} token</title></rect>`
      + `<text x="${i * w + w / 2}" y="88" text-anchor="middle">${String(g.giorno).slice(8, 10)}</text>`;
  }).join('');
  return el('div.riquadro', {}, s);
}

export async function paginaUtilizzo({ ridisegna }) {
  const ora = Date.now();
  const da = ora - filtri.giorni * GIORNO;
  const [quote, u] = await Promise.all([caricaQuote(), api.utilizzo({
    da_ms: da, a_ms: ora, provider_id: filtri.provider || undefined, assistente_id: filtri.assistente || undefined,
  })]);
  // Finché il server non filtra (lo si vede dalle righe che tornano), si filtra qui; le tabelle che
  // non si possono ricavare dal filtro restano nascoste con una nota, mai intere e fuorvianti.
  const tutteP = u?.per_provider || [];
  const tutteA = u?.per_assistente || [];
  const serverFiltraA = !filtri.assistente || tutteA.every((r) => r.assistente_id === filtri.assistente);
  const serverFiltraP = !filtri.provider || tutteP.every((r) => r.provider_id === filtri.provider);
  const perAssistente = serverFiltraP ? tutteA.filter((r) => !filtri.assistente || r.assistente_id === filtri.assistente) : [];
  const perModello = serverFiltraA ? (u?.per_modello || []).filter((r) => !filtri.provider || !r.provider_id || r.provider_id === filtri.provider) : [];
  const perProvider = serverFiltraA ? tutteP.filter((r) => !filtri.provider || r.provider_id === filtri.provider) : [];
  const perGiorno = serverFiltraA && serverFiltraP ? (u?.per_giorno || []) : [];
  const parziale = !serverFiltraA || !serverFiltraP;
  const cambia = (k) => (v) => { filtri = { ...filtri, [k]: k === 'giorni' ? Number(v) : v }; ridisegna(); };
  const assistenti = [...stato.assistenti.values()];
  const nomeA = (r) => r.nome || stato.assistenti.get(r.assistente_id)?.nome || r.assistente_id || '—';
  const totTurni = (filtri.assistente && !filtri.provider ? perAssistente : perProvider).reduce((s, r) => s + Number(r.turni || 0), 0);
  return [
    el('div.riquadro-titolo', {}, 'Quote'),
    el('div.riquadro', {}, quote.some((q) => providerAcceso(q.provider_id)) ? quote.filter((q) => providerAcceso(q.provider_id)).flatMap((q) => {
      const d = quotaDi(q.provider_id)?.dettaglio || {};
      const finestre = Object.entries(d).filter(([, v]) => v && typeof v === 'object' && 'percentuale_usata' in v);
      const barre = barreQuota(q.provider_id, q.fonte);
      return [el('div.provider-riga.quota-utilizzo', {},
        el('span.testi', {}, el('strong', {}, q.nome || nomeProvider(q.provider_id)),
          el('small', {}, q.rilevata_ms ? `letta ${quandoBreve(q.rilevata_ms)} · fonte ${q.fonte === 'client' ? 'client' : 'stima'}` : 'nessuna rilevazione'),
          finestre.length && !barre && !quotaScaduta(q) ? el('small', { style: 'display:block' }, finestre.map(([k, v]) => `${{ five_hour: '5 ore', seven_day: '7 giorni' }[k] || k}: ${numero(v.percentuale_usata)} %`).join(' · ')) : null),
        anelloGrande(q.provider_id)), barre];
    }) : vuoto('Nessun provider attivo.')),
    el('div.filtri', { role: 'group', 'aria-label': 'Filtri' },
      tendina('Filtro del periodo', [[1, 'Oggi'], [7, '7 giorni'], [30, '30 giorni'], [90, '90 giorni']], filtri.giorni, cambia('giorni')),
      tendina('Filtro del provider', [['', 'Tutti i provider'], ...providerAccesi().map((p) => [p.id, p.nome])], filtri.provider, cambia('provider')),
      tendina('Filtro dell\'assistente', [['', 'Tutti gli assistenti'], ...assistenti.map((a) => [a.id, a.nome])], filtri.assistente, cambia('assistente'))),
    el('div.riquadro-titolo', {}, `Ultimi ${filtri.giorni} ${filtri.giorni === 1 ? 'giorno' : 'giorni'} · ${numero(totTurni)} turni`),
    parziale ? nota('Il server non filtra ancora questi conteggi: qui restano solo le tabelle che rispettano il filtro; le altre tornano quando il server li divide.') : null,
    barreGiorni(perGiorno),
    perProvider.length ? tabella(['Provider', 'Turni', 'Token entrata', 'Token uscita', 'Costo stimato'],
      perProvider.map((r) => [nomeProvider(r.provider_id), numero(r.turni), compatto(r.token_ingresso), compatto(r.token_uscita), costo(r.costo_usd)])) : (serverFiltraA ? vuoto('Nessun turno nel periodo.') : null),
    perAssistente.length ? el('div.riquadro-titolo', {}, 'Per assistente') : null,
    perAssistente.length ? tabella(['Assistente', 'Turni', 'Token', 'Costo stimato'], perAssistente.map((r) => [nomeA(r), numero(r.turni), compatto(tot(r)), costo(r.costo_usd)])) : null,
    perModello.length ? el('div.riquadro-titolo', {}, 'Per modello') : null,
    perModello.length ? tabella(['Modello', 'Turni', 'Token', 'Costo stimato'], perModello.map((r) => [r.etichetta || r.modello_id || '—', numero(r.turni), compatto(tot(r)), costo(r.costo_usd)])) : null,
    el('p.nota', {}, 'Niente fatturazione, niente crediti: sono gli abbonamenti. Il costo è quello stimato dal client, in dollari.'),
  ];
}
