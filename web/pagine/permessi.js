// Permessi (Layout sez. 2.3, CONTRATTO-2D sez. 4): richieste aperte con risposta, concessioni
// stabili con revoca, regole di revisione «quando… → consenti/blocca» (al massimo 20, 1000 caratteri).

import { api, elenco, testoErrore } from '../api.js';
import { icona } from '../icone.js';
import { chiediConferma } from '../menu.js';
import { richiesteAperte, schedaRichiesta } from '../richieste.js';
import { stato } from '../stato.js';
import { interruttore, nota, tastoIcona, vuoto } from '../ui.js';
import { avvisa, el, quandoBreve } from '../util.js';

const MASSIMO = 20;
const AZIONI = { 'exec.run': 'Comando', permesso_locale: 'Comando', cassaforte: 'Cassaforte', revisione_comando: 'Comando rivisto', messaggio: 'Messaggi a', collegamento: 'Collegamento', 'run-command': 'Comando', strumento_mcp: 'Strumento', cassaforte_chiedi: 'Cassaforte' };

function editorRegole(regole, ridisegna) {
  let righe = regole.map((r) => ({ tipo: r.tipo === 'blocca' ? 'blocca' : 'consenti', testo: r.testo || '', attiva: r.attiva !== false && r.attiva !== 0 }));
  const lista = el('div.regole');
  const contatore = el('small.contatore');
  const salva = el('button.tasto.primario.piccolo', { type: 'button' }, 'Salva le regole');
  const aggiungi = el('button.tasto.piccolo', { type: 'button' }, icona('piu', 14), 'Aggiungi regola');
  const disegna = () => {
    lista.replaceChildren(...righe.map((r, i) => el('div.regola', {},
      el('span.regola-quando', {}, 'Quando vuole'),
      el('input', { value: r.testo, maxlength: 1000, placeholder: 'es. cancellare file', 'aria-label': `Regola ${i + 1}: quando l'assistente vuole`, oninput: (ev) => { r.testo = ev.target.value; } }),
      el('select', { 'aria-label': `Regola ${i + 1}: cosa fare`, onchange: (ev) => { r.tipo = ev.target.value; } },
        el('option', { value: 'consenti', selected: r.tipo === 'consenti' }, '→ consenti'), el('option', { value: 'blocca', selected: r.tipo === 'blocca' }, '→ blocca')),
      interruttore(`Regola ${i + 1} attiva`, r.attiva, (si) => { r.attiva = si; }),
      tastoIcona('cestino', `Togli la regola ${i + 1}`, () => { righe.splice(i, 1); disegna(); }))));
    if (!righe.length) lista.append(vuoto('Nessuna regola: ogni azione segue le concessioni e il permesso della macchina.'));
    contatore.textContent = `${righe.length} di ${MASSIMO}`;
    aggiungi.disabled = righe.length >= MASSIMO;
  };
  aggiungi.onclick = () => { if (righe.length < MASSIMO) { righe.push({ tipo: 'consenti', testo: '', attiva: true }); disegna(); lista.querySelector('.regola:last-child input')?.focus(); } };
  salva.onclick = async () => {
    const pulite = righe.map((r) => ({ ...r, testo: r.testo.trim() })).filter((r) => r.testo);
    salva.disabled = true;
    try { await api.scriviRegole(pulite); avvisa('Regole salvate'); righe = pulite; disegna(); } catch (e) { avvisa(testoErrore(e)); } finally { salva.disabled = false; }
  };
  disegna();
  return el('div.riquadro', {}, lista, el('div.riga-tasti', {}, aggiungi, contatore, el('span.spazio'), salva));
}

export async function paginaPermessi({ ridisegna }) {
  const [aperte, perm, reg, voci] = await Promise.all([
    richiesteAperte(),
    api.permessi().catch(() => ({ permessi: [] })),
    api.regole().catch(() => ({ regole: [] })),
    api.cassaforte().then((r) => elenco(r, 'voci')).catch(() => []),
  ]);
  const titoli = new Map(voci.map((v) => [v.id, v.titolo]));
  const risorsa = (p) => titoli.get(p.risorsa) || titoli.get(p.destinatario) || p.risorsa;
  const concessioni = elenco(perm, 'permessi');
  const nomeA = (id) => stato.assistenti.get(id)?.nome;
  return [
    el('div.riquadro-titolo', {}, `Richieste aperte${aperte.length ? ` (${aperte.length})` : ''}`),
    aperte.length ? el('div.richieste-pagina', {}, aperte.map((r) => schedaRichiesta(r, { conAssistente: true }))) : el('div.riquadro', {}, vuoto('Nessuna richiesta in attesa.')),
    el('div.riquadro-titolo', {}, 'Concessioni stabili («Sempre»)'),
    el('div.riquadro.elenco', {}, concessioni.length ? concessioni.map((p) => el('div.riga-elenco', {},
      el('span.riga-testi', {}, el('strong', {}, `${AZIONI[p.azione] || p.azione || 'Azione'}${p.destinatario && !titoli.has(p.destinatario) ? ` ${p.destinatario}` : ''}`),
        el('small', {}, [risorsa(p) ? `«${risorsa(p)}»` : null, p.macchina_id ? `su ${p.macchina_id}` : null, nomeA(p.assistente_id) ? `per ${nomeA(p.assistente_id)}` : 'per tutti', p.creato_ms ? quandoBreve(p.creato_ms) : null].filter(Boolean).join(' · '))),
      el('button.tasto.piccolo.pericolo', { type: 'button', onclick: async () => {
        if (!await chiediConferma({ titolo: 'Revocare la concessione?', testo: 'La prossima volta l\'assistente dovrà chiedere di nuovo.', conferma: 'Revoca', pericolo: true })) return;
        try { await api.revocaPermesso(p.id); avvisa('Revocata'); ridisegna(); } catch (e) { avvisa(testoErrore(e)); }
      } }, 'Revoca'))) : vuoto('Nessuna concessione stabile.')),
    el('div.riquadro-titolo', {}, 'Regole di revisione'),
    editorRegole(elenco(reg, 'regole'), ridisegna),
    nota('Le regole si confrontano con il comando e la sua descrizione prima di eseguire. «Blocca» vince su «consenti».'),
  ];
}
