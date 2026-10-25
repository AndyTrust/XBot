// Schede del foglio dell'assistente: Modello e voce (macchina, Chrome del Mac, provider e modello
// dal foglio di scelta, pensiero, voce: legate ai dati del foglio, si salvano con «Salva»), Profilo (editor del .md con il suo «Salva»), Collegamenti (caselle, salvate subito).

import { api, elenco, testoErrore } from '../api.js';
import { etichettaMacchina, gruppiScelta, inLinea, leggiMacchine, macchinaMac, macchinaServer, macchine, valoreScelta } from '../macchine.js';
import { bottoneScelta, etichettaLivello } from '../scelta.js';
import { nomeProvider, providerAuto, stato } from '../stato.js';
import { interruttore, nota, vuoto } from '../ui.js';
import { avvisa, el } from '../util.js';
import { parla } from '../voce.js';
import { fraseNonRiceve, providerDiAssistente } from './collegamenti-fogli.js';

const LINGUE = [['it', 'Italiano'], ['en', 'Inglese'], ['es', 'Spagnolo'], ['fr', 'Francese'], ['de', 'Tedesco']];

function tendinaLegata(etichetta, dati, chiave, opzioni, quandoCambia) {
  const s = el('select', { onchange: (ev) => { dati[chiave] = ev.target.value; quandoCambia?.(ev.target.value); } },
    opzioni.map(([v, t, off]) => el('option', { value: v, selected: String(v) === String(dati[chiave] ?? ''), disabled: off }, t)));
  return [el('label.campo-modulo', {}, etichetta, s), s];
}

export function schedaModelloVoce(dati) {
  const auto = providerAuto();
  // Macchina predefinita: «Automatica» = la VPS, o il Mac per i client che stanno solo lì.
  const selMacchina = el('select', { onchange: (ev) => { dati.macchina_predefinita_id = ev.target.value; riempiScelta(); } });
  const boxScelta = el('div.scelta-pannello');
  const selLivello = el('select', { onchange: (ev) => { dati.livello_ragionamento = ev.target.value; } });
  const avvisoChrome = el('small', {});
  const chrome = interruttore('Usa Chrome del Mac', !!dati.usa_chrome, (si) => {
    dati.usa_chrome = si;
    if (si) {
      const mac = macchinaMac();
      if (mac) dati.macchina_predefinita_id = mac.id;
      if (dati.provider_predefinito_id && dati.provider_predefinito_id !== 'claude_code') { dati.provider_predefinito_id = 'claude_code'; dati.modello_predefinito_id = ''; dati.livello_ragionamento = ''; }
    }
    riempiMacchine();
  });
  function riempiMacchine() {
    const tutte = macchine.filter((m) => !['android', 'ios'].includes(m.genere));
    selMacchina.replaceChildren(el('option', { value: '' }, 'Automatica (VPS, o Mac per i suoi client)'),
      ...tutte.map((m) => el('option', { value: m.id, selected: m.id === dati.macchina_predefinita_id }, `${etichettaMacchina(m)}${inLinea(m) ? '' : ' (spenta)'}`)));
    selMacchina.disabled = !!dati.usa_chrome;
    const mac = macchinaMac();
    avvisoChrome.textContent = dati.usa_chrome
      ? (mac ? `Turni sul ${etichettaMacchina(mac)} con Claude Code e l'estensione Claude in Chrome${inLinea(mac) ? '' : ': ora è spento, i turni aspettano'}.` : 'Nessun Mac collegato: collegalo dalle impostazioni, sezione Computer.')
      : 'Apre Chrome del Mac con le tue sessioni: solo con Claude Code.';
    riempiScelta();
  }
  // Provider e modello predefiniti: lo stesso foglio di scelta della barra di scrittura, con
  // anche le macchine spente. Scegliere una voce di un'altra macchina cambia la macchina predefinita.
  async function riempiScelta() {
    const server = macchinaServer();
    const prov = dati.provider_predefinito_id || '';
    const mod = dati.modello_predefinito_id || '';
    const valore = valoreScelta(dati.macchina_predefinita_id || server?.id || '', prov, mod);
    let voci = [];
    try { voci = (await gruppiScelta({ tutte: true })).flatMap((g) => g.voci); } catch { voci = []; }
    const voce = voci.find((v) => v.valore === valore);
    const simile = voce || voci.find((v) => (v.provider?.id || '') === prov && (v.modello?.id || '') === mod);
    const testo = voce?.testo || (prov ? `${nomeProvider(prov)} · ${simile?.etichetta || mod || 'predefinito'} (non su questa macchina)` : `Auto (${nomeProvider(auto)})`);
    boxScelta.replaceChildren(bottoneScelta({ testo, valore, tutte: true, etichetta: 'Provider e modello predefiniti', quandoSceglie: scegliPredefinito }));
    riempiLivelli(simile?.livelli || []);
  }
  function scegliPredefinito(v) {
    if (dati.usa_chrome && v.provider && v.provider.id !== 'claude_code') { avvisa('Con «Usa Chrome del Mac» serve Claude Code'); return; }
    dati.provider_predefinito_id = v.provider?.id || '';
    dati.modello_predefinito_id = v.modello?.id || '';
    if (!(v.livelli || []).includes(dati.livello_ragionamento)) dati.livello_ragionamento = '';
    if (!dati.usa_chrome) dati.macchina_predefinita_id = !v.macchina?.id || v.macchina === macchinaServer() ? '' : v.macchina.id;
    riempiMacchine();
  }
  function riempiLivelli(l) {
    selLivello.replaceChildren(el('option', { value: '' }, 'predefinito'), ...l.map((x) => el('option', { value: x, selected: x === dati.livello_ragionamento }, etichettaLivello(x))));
    selLivello.disabled = !l.length;
  }
  riempiMacchine();
  if (!macchine.length) leggiMacchine().then(riempiMacchine);
  const selVoce = el('select', { onchange: (ev) => { dati.voce_id = ev.target.value; } }, el('option', { value: '' }, 'Voce predefinita'));
  api.voci().then((r) => {
    for (const v of elenco(r, 'voci')) selVoce.append(el('option', { value: v.id, selected: v.id === dati.voce_id }, v.nome));
  }).catch(() => {});
  const [cLingua] = tendinaLegata('Lingua (dettato e voce)', dati, 'voce_lingua', LINGUE);
  const velocita = el('input', { type: 'range', min: 0.7, max: 1.6, step: 0.05, value: dati.voce_velocita || 1, 'aria-label': 'Velocità della voce' });
  const valoreV = el('output', {}, `${Number(dati.voce_velocita || 1).toFixed(2).replace('.', ',')}×`);
  velocita.addEventListener('input', () => { dati.voce_velocita = Number(velocita.value); valoreV.textContent = `${Number(velocita.value).toFixed(2).replace('.', ',')}×`; });
  return [
    el('label.campo-modulo', {}, 'Macchina predefinita', selMacchina),
    el('div.campo-modulo', {}, el('span.riga-interruttore', {}, el('span', {}, 'Usa Chrome del Mac'), chrome), avvisoChrome),
    el('div.campo-modulo', {}, 'Provider e modello predefiniti', boxScelta),
    el('label.campo-modulo', {}, 'Pensiero', selLivello),
    el('label.campo-modulo', {}, 'Voce', selVoce),
    el('label.campo-modulo', {}, 'Velocità', el('span.con-valore', {}, velocita, valoreV)),
    cLingua,
    el('div.campo-modulo.largo', {}, el('button.tasto.piccolo', { type: 'button', onclick: () => parla(`Ciao, sono ${dati.nome || 'il tuo assistente'}. Questa è la mia voce.`) }, 'Prova la voce'),
      nota('La prova usa la voce salvata: salva prima di provarne una nuova.')),
  ];
}

export function schedaProfilo(id) {
  const area = el('textarea.editor', { rows: 18, 'aria-label': 'Profilo dell\'assistente (Markdown)', disabled: true }, '');
  const percorso = el('small', {}, 'Carico…');
  const salva = el('button.tasto.primario.piccolo', { type: 'button', disabled: true }, 'Salva il profilo');
  let originale = '';
  api.profilo(id).then((p) => {
    originale = p?.testo || '';
    area.value = originale;
    area.disabled = false;
    salva.disabled = false;
    percorso.textContent = p?.percorso ? `File: ${p.percorso}` : '';
  }).catch((e) => { percorso.textContent = testoErrore(e); });
  salva.onclick = async () => {
    salva.disabled = true;
    try { await api.scriviProfilo(id, area.value); originale = area.value; avvisa('Profilo salvato'); } catch (e) { avvisa(testoErrore(e)); } finally { salva.disabled = false; }
  };
  return [
    el('p.nota', {}, '«Chi sei e cosa sai fare»: entra nel testo di sistema di ogni turno, insieme alle abilità attive e alla memoria.'),
    area,
    el('div.riga-tasti', {}, percorso, el('span.spazio'), salva),
  ];
}

export function schedaCollegamenti(id) {
  const box = el('div.riquadro.elenco', {}, el('p.nota', {}, 'Carico…'));
  // 2.L: un collegamento acceso vale per tutti; la casella tolta mette questo assistente fra gli `esclusi`.
  const prov = providerDiAssistente(stato.assistenti.get(id));
  api.collegamenti().then((r) => {
    const tutti = elenco(r, 'collegamenti');
    if (!tutti.length) { box.replaceChildren(vuoto('Nessun collegamento: si aggiungono dalle impostazioni, sezione Collegamenti.')); return; }
    box.replaceChildren(...tutti.map((c) => el(`label.riga-elenco.casella${prov.riceve ? '' : '.disattiva'}`, {},
      el('input', { type: 'checkbox', checked: !(c.esclusi || []).includes(id), onchange: async (ev) => {
        const s = new Set(c.esclusi || []);
        if (ev.target.checked) s.delete(id); else s.add(id);
        try { await api.esclusiCollegamento(c.id, [...s]); c.esclusi = [...s]; avvisa(ev.target.checked ? `«${c.nome}» acceso per questo assistente` : `«${c.nome}» spento per questo assistente`); } catch (e) { ev.target.checked = !ev.target.checked; avvisa(testoErrore(e)); }
      } }),
      el('span.riga-testi', {}, el('strong', {}, c.nome), el('small', {}, `${c.abilitato ? 'acceso' : 'spento per tutti'} · ${(c.strumenti || []).length} strumenti`)))));
  }).catch((e) => box.replaceChildren(vuoto(testoErrore(e))));
  return [prov.riceve ? null : el('p.avviso-forte.non-riceve', {}, fraseNonRiceve(prov)), box,
    nota('Un collegamento acceso vale per tutti gli assistenti: togli la casella per spegnerlo solo su questo.')];
}
