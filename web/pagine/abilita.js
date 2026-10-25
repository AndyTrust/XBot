// Abilità di un assistente (CONTRATTO-2D sez. 2): file `.md` con `descrizione:`; «＋», attiva o
// disattiva, modifica del testo, elimina. Le usano il pannello e le impostazioni dell'assistente.

import { api, elenco, testoErrore } from '../api.js';
import { chiediConferma } from '../menu.js';
import { campo, foglio, interruttore, tastoIcona, titolo, vuoto } from '../ui.js';
import { bus } from '../stato.js';
import { avvisa, el } from '../util.js';

/** Il foglio per una abilità nuova o da modificare. */
export async function foglioAbilita(assistenteId, ab = null, dopo = () => {}) {
  let testo = ab?.testo ?? '';
  if (ab && ab.testo === undefined) {
    try { testo = (await api.unaAbilita(assistenteId, ab.nome))?.testo ?? ''; } catch (e) { avvisa(`Testo non letto: ${testoErrore(e)}`); }
  }
  foglio({
    titolo: ab ? `Abilità «${ab.nome}»` : 'Nuova abilità',
    conferma: ab ? 'Salva' : 'Crea',
    corpo: [
      ab ? null : campo('Nome (diventa il nome del file)', { name: 'nome', required: true, maxlength: 60, placeholder: 'es. report-pdf (minuscole, cifre, - e _)' }, true),
      campo('Descrizione (quando usarla)', { name: 'descrizione', valore: ab?.descrizione || '', maxlength: 300 }, true),
      campo('Testo dell\'abilità (Markdown)', { name: 'testo', valore: testo, rows: 12, class: 'editor' }, true),
    ],
    invia: async (m) => {
      const v = Object.fromEntries(new FormData(m));
      try {
        if (ab) await api.modificaAbilita(assistenteId, ab.nome, { descrizione: v.descrizione, testo: v.testo });
        else {
          const nome = v.nome.trim().toLocaleLowerCase('it').normalize('NFD').replace(/[\u0300-\u036f]/g, '').replace(/\s+/g, '-').replace(/[^a-z0-9_-]/g, '').slice(0, 60);
          if (!nome) throw new Error('Il nome vuole lettere, cifre, - o _.');
          await api.creaAbilita(assistenteId, { nome, descrizione: v.descrizione, testo: v.testo });
        }
      } catch (e) { throw new Error(testoErrore(e)); }
      avvisa(ab ? 'Abilità salvata' : 'Abilità creata');
      dopo();
      bus.emetti('assistente:aggiornato', assistenteId);
    },
  });
}

export function sezioneAbilita(assistenteId, { compatta = false } = {}) {
  const lista = el('div.riquadro.elenco');
  const nodo = el('div.sezione-pannello', {}, titolo('Abilità', tastoIcona('piu', 'Nuova abilità', () => foglioAbilita(assistenteId, null, carica))), lista);
  async function carica() {
    let ab = [];
    try { ab = elenco(await api.abilita(assistenteId), 'abilita'); } catch (e) { lista.replaceChildren(vuoto(testoErrore(e))); return; }
    if (!ab.length) { lista.replaceChildren(vuoto('Nessuna abilità. Aggiungila con ＋.')); return; }
    lista.replaceChildren(...ab.map((x) => el('div.riga-elenco', {},
      el('span.riga-testi', {}, el('strong', {}, x.nome), el('small', {}, x.descrizione || x.percorso || '')),
      tastoIcona('matita', `Modifica «${x.nome}»`, () => foglioAbilita(assistenteId, x, carica)),
      compatta ? null : tastoIcona('cestino', `Elimina «${x.nome}»`, async () => {
        if (!await chiediConferma({ titolo: `Eliminare «${x.nome}»?`, testo: 'Il file dell\'abilità viene tolto.', conferma: 'Elimina', pericolo: true })) return;
        try { await api.togliAbilita(assistenteId, x.nome); carica(); bus.emetti('assistente:aggiornato', assistenteId); } catch (e) { avvisa(testoErrore(e)); }
      }),
      interruttore(`Abilità «${x.nome}» attiva`, x.attiva, async (si, n) => {
        try { await api.modificaAbilita(assistenteId, x.nome, { attiva: si }); } catch (e) { n.checked = !si; avvisa(testoErrore(e)); }
      }))));
  }
  carica();
  return nodo;
}
