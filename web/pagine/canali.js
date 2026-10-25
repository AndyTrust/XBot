// Canali (Layout sez. 2.6, CONTRATTO-2D sez. 6): chi usa la app da fuori come
// client MCP. Nuovo canale con il token mostrato una volta e il tasto Copia, istruzioni per i client
// esterni, rigenera, revoca, strumenti concessi, ultime chiamate; i revocati nascosti (CONTRATTO-2I
// sez. 4.3) con «mostra i revocati» ed «Elimina».

import { api, elenco, testoErrore } from '../api.js';
import { icona } from '../icone.js';
import { chiediConferma } from '../menu.js';
import { NOME_APP } from '../nome.js';
import { stato } from '../stato.js';
import { campo, daCopiare, foglio, interruttore, nota, tastino, vuoto } from '../ui.js';
import { avvisa, durata, el, quandoBreve, testoDa } from '../util.js';

export const STRUMENTI = [
  ['leggi_memoria', 'Leggi la memoria'], ['cerca_memoria', 'Cerca nella memoria'], ['scrivi_quaderno', 'Scrivi nel quaderno'],
  ['elenco_assistenti', 'Elenco degli assistenti'], ['invia_a_assistente', 'Scrivi a un assistente e aspetta la risposta'],
  ['stato_quote', 'Stato delle quote'], ['cassaforte_elenco', 'Titoli della cassaforte (mai i valori)'],
  ['esegui_routine', 'Fai partire una routine'], ['esito_routine', 'Esito di una routine'],
];

const urlMcp = () => `${location.origin}/mcp`;
let mostraRevocati = false;

/** Il token appena creato o rigenerato: si vede adesso e mai più. */
function mostraToken(nome, token) {
  foglio({
    titolo: `Token di «${nome}»`, unaColonna: true,
    corpo: [
      el('p.avviso-forte.largo', {}, icona('avviso', 16), 'Copialo adesso: il token si vede una volta sola.'),
      el('div.largo', {}, el('small', {}, 'Token'), daCopiare(token)),
      el('div.largo', {}, el('small', {}, 'Indirizzo del server MCP'), daCopiare(urlMcp())),
      el('div.largo', {}, el('small', {}, 'Intestazione Authorization'), daCopiare(`Bearer ${token}`)),
      el('small.largo', {}, 'Grok Bot'), istruzioni(),
      el('small.largo', {}, 'Reason'), istruzioniReason(),
    ],
  });
}

function istruzioni() {
  return el('ol.istruzioni.largo', {},
    el('li', {}, 'In Grok Bot apri «Connetti app» → «Add a custom MCP».'),
    el('li', {}, `Server URL: ${urlMcp()}`),
    el('li', {}, 'Authorization header: «Bearer » seguito dal token.'),
    el('li', {}, `Salva: Grok Bot vede gli strumenti concessi qui e ogni chiamata compare nelle «Ultime chiamate».`));
}

/** Reason: i server MCP si aggiungono dal suo sito (resoconto reason.md sez. 6.2), trasporto HTTP. */
function istruzioniReason() {
  return el('ol.istruzioni.largo', {},
    el('li', {}, 'Apri Reason nel browser e vai nella pagina dei server MCP.'),
    el('li', {}, 'Aggiungi un server di tipo HTTP.'),
    el('li', {}, `URL: ${urlMcp()}`),
    el('li', {}, 'Intestazione: Authorization con valore «Bearer » seguito dal token.'),
    el('li', {}, 'Salva e accendilo: le chiamate di Reason compaiono nelle «Ultime chiamate» di questo canale.'));
}

function foglioCanale(dopo) {
  const assistenti = [...stato.assistenti.values()].filter((a) => !a.archiviato_ms);
  foglio({
    titolo: 'Nuovo canale', conferma: 'Crea e mostra il token',
    corpo: [
      campo('Nome', { name: 'nome', required: true, maxlength: 80, placeholder: 'es. Grok Bot' }, true),
      campo('Assistente che risponde', { name: 'assistente_id', opzioni: [['', 'Nessuno (solo lettura)'], ...assistenti.map((a) => [a.id, a.nome])] }, true),
      el('fieldset.caselle.largo', {}, el('legend', {}, 'Strumenti concessi'),
        ...STRUMENTI.map(([k, t]) => el('label.casella', {}, el('input', { type: 'checkbox', name: `strumento:${k}`, checked: ['leggi_memoria', 'cerca_memoria', 'stato_quote', 'elenco_assistenti'].includes(k) }), el('span', {}, t, el('code', {}, ` ${k}`))))),
    ],
    invia: async (m) => {
      const v = Object.fromEntries(new FormData(m));
      const strumenti = STRUMENTI.map(([k]) => k).filter((k) => v[`strumento:${k}`]);
      let r;
      try { r = await api.creaCanale({ nome: v.nome.trim(), assistente_id: v.assistente_id || null, strumenti }); } catch (e) { throw new Error(testoErrore(e)); }
      dopo();
      setTimeout(() => mostraToken(v.nome.trim(), r.token), 0);
    },
  });
}

function riga(c, ridisegna) {
  const chiamate = el('div.voce-extra', { hidden: true });
  const nomeA = stato.assistenti.get(c.assistente_id)?.nome;
  const apriChiamate = async () => {
    chiamate.hidden = !chiamate.hidden;
    if (chiamate.hidden) return;
    chiamate.replaceChildren(el('p.nota', {}, 'Carico…'));
    try {
      const l = elenco(await api.chiamate(c.id, 20), 'chiamate');
      chiamate.replaceChildren(l.length ? el('div.elenco', {}, l.map((x) => el('div.riga-elenco', {},
        el(`span.esito.${x.esito === 'ok' || x.esito === 'riuscito' ? 'ok' : 'no'}`, {}, x.esito === 'ok' || x.esito === 'riuscito' ? '✓' : '✗'),
        el('span.riga-testi', {}, el('code', {}, `${x.strumento}(${testoDa(x.argomenti_json).replace(/\s+/g, ' ').slice(0, 60)})`),
          el('small', {}, `${quandoBreve(x.ms)} · ${durata(x.durata_ms)}`))))) : vuoto('Nessuna chiamata.'));
    } catch (e) { chiamate.replaceChildren(vuoto(testoErrore(e))); }
  };
  return el(`div.voce-cassaforte${c.revocato ? '.revocato' : ''}`, {},
    el('div.riga-elenco', {},
      el('span.icona-tonda', {}, icona('canali', 16)),
      el('span.riga-testi', {}, el('strong', {}, c.nome, c.revocato ? el('span.etichetta', {}, 'revocato') : null),
        el('small', {}, [`client MCP`, c.ultimo_uso_ms ? `ultimo uso ${quandoBreve(c.ultimo_uso_ms)}` : 'mai usato', `${c.chiamate_oggi || 0} chiamate oggi`, nomeA ? `risponde ${nomeA}` : null].filter(Boolean).join(' · ')),
        el('small', {}, `strumenti: ${(c.strumenti || []).join(', ') || 'nessuno'}`))),
    el('div.riga-tasti', {},
      tastino('Chiamate', 'sessioni', apriChiamate),
      tastino('Rigenera token', 'aggiorna', async () => {
        if (!await chiediConferma({ titolo: `Rigenerare il token di «${c.nome}»?`, testo: 'Il token vecchio smette di funzionare subito: va incollato quello nuovo nel client.', conferma: 'Rigenera' })) return;
        try { const r = await api.rigeneraCanale(c.id); ridisegna(); mostraToken(c.nome, r.token); } catch (e) { avvisa(testoErrore(e)); }
      }),
      c.revocato ? null : tastino('Revoca', 'chiudi', async () => {
        if (!await chiediConferma({ titolo: `Revocare «${c.nome}»?`, testo: 'Il client non potrà più usare la app finché non rigeneri il token.', conferma: 'Revoca', pericolo: true })) return;
        try { await api.revocaCanale(c.id); avvisa('Revocato: lo trovi con «mostra i revocati»'); ridisegna(); } catch (e) { avvisa(testoErrore(e)); }
      }, { class: 'tasto piccolo pericolo' }),
      c.revocato ? tastino('Elimina', 'cestino', async () => {
        if (!await chiediConferma({ titolo: `Eliminare «${c.nome}»?`, testo: 'Il canale sparisce con le sue ultime chiamate. Non si torna indietro.', conferma: 'Elimina', pericolo: true })) return;
        try { await api.eliminaCanale(c.id); avvisa('Eliminato'); ridisegna(); } catch (e) { avvisa(testoErrore(e)); }
      }, { class: 'tasto piccolo pericolo' }) : null),
    chiamate);
}

export async function paginaCanali({ ridisegna }) {
  const tutti = elenco(await api.canali(), 'canali');
  const revocati = tutti.filter((c) => c.revocato).length;
  const canali = mostraRevocati ? tutti : tutti.filter((c) => !c.revocato);
  return [
    el('div.riga-tasti.testa-pagina', {}, el('span.nota', {}, `Chi può usare ${NOME_APP} da fuori`), el('span.spazio'),
      el('button.tasto.primario', { type: 'button', onclick: () => foglioCanale(ridisegna) }, icona('piu', 14), 'Nuovo canale')),
    revocati ? el('div.coppia.mostra-revocati', {}, el('span', {}, `Mostra i revocati (${revocati})`),
      interruttore('Mostra i revocati', mostraRevocati, (si) => { mostraRevocati = si; ridisegna(); })) : null,
    el('div.riquadro.elenco', {}, canali.length ? canali.map((c) => riga(c, ridisegna)) : vuoto(revocati ? 'Nessun canale attivo.' : 'Nessun canale. Creane uno per Grok Bot o Reason.')),
    el('div.riquadro-titolo', {}, 'Come si collega Grok Bot'),
    el('div.riquadro', {}, istruzioni(), el('div.largo', {}, el('small', {}, 'Indirizzo del server MCP'), daCopiare(urlMcp()))),
    el('div.riquadro-titolo', {}, 'Come si collega Reason'),
    el('div.riquadro', {}, istruzioniReason(), el('p.nota', {}, 'Il token è quello mostrato quando crei il canale (o lo rigeneri).')),
    nota('Le chiamate che eseguono lavori passano dalle stesse richieste di permesso della chat.'),
  ];
}
