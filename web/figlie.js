// Sessioni figlie e lavagna comune: la riga di stato nel padre, il ritorno dalla figlia e la
// sezione «Lavagna» del pannello (CONTRATTO-ANNIDATE). Il server scrive la riga `evento` con
// `codice: figlia` e la riscrive a ogni cambio di stato; le richieste della figlia stanno nella riga.

import { api, testoErrore } from './api.js';
import { icona } from './icone.js';
import { schedaRichiesta } from './richieste.js';
import { assistenteDi, bus } from './stato.js';
import { avvisa, el, quandoBreve } from './util.js';

export const STATI_FIGLIA = {
  al_lavoro: 'al lavoro',
  in_attesa: 'tocca a te',
  finita: 'finita',
  fallita: 'fallita',
};

/** La riga del padre: titolo, stato, risultato o richieste, e «Apri». */
export function rigaFiglia(r, c) {
  const nome = assistenteDi(c.assistente_id).nome || '';
  const chiusa = c.stato === 'finita' || c.stato === 'fallita';
  const nodo = el(`div.riga-figlia.${c.stato}`, { dati: { id: r.id, figlia: c.figlia_id } },
    el('div.riga-figlia-testa', {},
      icona('sessioni', 15),
      el('strong', {}, c.titolo || 'Chat figlia'),
      nome ? el('small', {}, `con ${nome}`) : null,
      el('span.stato-figlia', { role: 'status' }, STATI_FIGLIA[c.stato] || c.stato),
      el('button.tasto.piccolo', { type: 'button', onclick: () => bus.emetti('apri:sessione', { assistente_id: c.assistente_id, sessione_id: c.figlia_id }) }, 'Apri')));
  if (c.riassunto) nodo.append(el('p.riassunto-figlia', {}, c.riassunto));
  else if (!chiusa) nodo.append(el('p.riassunto-figlia.in-corso', {}, c.stato === 'in_attesa' ? 'Aspetta una tua risposta.' : 'Sta lavorando…'));
  for (const q of c.richieste || []) {
    nodo.append(schedaRichiesta({
      richiesta_id: q.id, tipo: q.tipo, carico: q.carico, stato: 'aperta',
      sessione_id: c.figlia_id, assistente_id: c.assistente_id, creata_ms: r.aggiornata_ms,
    }, { quandoRisolta: () => bus.emetti('barra') }));
  }
  return nodo;
}

/** Nella figlia: un tasto per tornare alla chat principale (accanto alla pillola). */
export function bannerPadre(s) {
  if (!s?.sessione_padre_id) return null;
  return el('button.tasto.piccolo.torna-padre', {
    type: 'button', title: 'Torna alla chat principale',
    onclick: async () => {
      try {
        const p = await api.sessione(s.sessione_padre_id);
        bus.emetti('apri:sessione', { assistente_id: p.assistente_id, sessione_id: p.id });
      } catch (e) { avvisa(testoErrore(e)); }
    },
  }, icona('rispondi', 14), 'Chat principale');
}

/** Il riquadro «Lavagna»: chi lavora adesso su cosa. Solo lettura, con «chiudi» per le voci dimenticate. */
export function sezioneLavagna() {
  const box = el('div.riquadro.elenco');
  const carica = async () => {
    let voci = [];
    try { voci = (await api.lavagna()).voci || []; } catch (e) { box.replaceChildren(el('p.nota', {}, testoErrore(e))); return; }
    if (!voci.length) { box.replaceChildren(el('p.nota', {}, 'Nessuno sta lavorando adesso.')); return; }
    box.replaceChildren(...voci.map((v) => el('div.voce-lavagna', {},
      el('div', {}, el('strong', {}, v.agente), el('span', {}, ` · ${v.progetto || '—'} · ${v.cosa}`)),
      el('small', {}, `da ${quandoBreve(v.inizio_ms)}${(v.file_json || []).length ? ` · ${(v.file_json || []).join(', ')}` : ''}${v.origine === 'registro' ? ' · dal registro' : ''}`),
      el('button.tasto.piccolo', { type: 'button', title: 'Chiudi questa voce', onclick: async () => { try { await api.chiudiVoce(v.id); carica(); } catch (e) { avvisa(testoErrore(e)); } } }, 'Chiudi'))));
  };
  carica();
  bus.su('sessione', () => { if (box.isConnected) carica(); });
  return [el('div.riquadro-titolo', {}, 'Lavagna'), box];
}

/** Apre una figlia dal campo di testo («in parallelo»): il compito è il testo scritto. */
export async function apriFigliaDa(sessioneId, compito, { menzione = '', titolo = '' } = {}) {
  const f = await api.apriFiglia(sessioneId, { compito, menzione, titolo });
  avvisa('Chat figlia aperta');
  return f;
}

