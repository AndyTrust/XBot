// Le impostazioni (Layout sez. 2): finestra sovrapposta con le sezioni. Qui la cornice, Generali e
// Modelli e quote; le altre pagine stanno in web/pagine/.

import { api, testoErrore } from './api.js';
import { icona } from './icone.js';
import { apriSovrapposta, chiediConferma } from './menu.js';
import { paginaAggiornamenti } from './pagine/aggiornamenti.js';
import { paginaCanali } from './pagine/canali.js';
import { paginaCassaforte } from './pagine/cassaforte.js';
import { paginaCollegamenti } from './pagine/collegamenti.js';
import { paginaModelli } from './pagine/modelli.js';
import { paginaComputer } from './pagine/computer.js';
import { paginaPermessi } from './pagine/permessi.js';
import { paginaSquadra } from './pagine/squadra.js';
import { paginaUtilizzo } from './pagine/utilizzo.js';
import { chiediNotifiche, statoNotifiche } from './push.js';
import { bus, CHIAVI, impostazione, scriviImpostazione, stato } from './stato.js';
import { coppia, interruttore, tendina } from './ui.js';
import { avvisa, el, fraQuanto, quandoBreve } from './util.js';
import { CHIAVE_MICROFONO, CHIAVE_PARLATA, microfoni } from './voce.js';

const SEZIONI = [
  ['generali', 'Generali', 'impostazioni'],
  ['squadra', 'Squadra', 'persona'],
  ['computer', 'Computer', 'computer'],
  ['permessi', 'Permessi', 'scudo'],
  ['cassaforte', 'Cassaforte', 'chiave'],
  ['modelli', 'Modelli e quote', 'anello'],
  ['canali', 'Canali', 'canali'],
  ['collegamenti', 'Collegamenti', 'presa'],
  ['utilizzo', 'Utilizzo', 'grafico'],
  ['aggiornamenti', 'Aggiornamenti', 'aggiorna'],
];

const PAGINE = {
  squadra: paginaSquadra, computer: paginaComputer, permessi: paginaPermessi, cassaforte: paginaCassaforte, canali: paginaCanali,
  collegamenti: paginaCollegamenti, modelli: paginaModelli, utilizzo: paginaUtilizzo, aggiornamenti: paginaAggiornamenti,
};

/** Apre le impostazioni su una sezione. `opzioni` passa alla pagina (es. {nuovo:'segreto'} alla cassaforte). */
export function apriImpostazioni(sezione = 'generali', { esci, opzioni } = {}) {
  let corrente = SEZIONI.some((s) => s[0] === sezione) ? sezione : 'generali';
  let opz = opzioni || {};
  const colonna = el('nav.impostazioni-colonna', { 'aria-label': 'Sezioni delle impostazioni' });
  const corpo = el('div.impostazioni-corpo');
  let giro = 0;
  // `ridisegna(opzioni)` ridisegna la pagina passandole opzioni nuove (es. {prova: id} ai collegamenti).
  const disegna = async (nuove) => {
    if (nuove && typeof nuove === 'object' && !(nuove instanceof Event)) opz = nuove;
    const mio = ++giro;
    colonna.replaceChildren(...SEZIONI.map(([id, nome, ic]) => el('button', {
      type: 'button', 'aria-current': id === corrente ? 'true' : 'false',
      onclick: () => { corrente = id; opz = {}; corpo.replaceChildren(); disegna(); },
    }, icona(ic, 16), el('span', {}, nome))));
    colonna.querySelector('[aria-current="true"]')?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    const titolo = SEZIONI.find((s) => s[0] === corrente)[1];
    if (!corpo.childElementCount) corpo.replaceChildren(el('h2', {}, titolo), el('p.nota', {}, 'Carico…'));
    try {
      const f = corrente === 'generali' ? () => generali(esci, disegna) : PAGINE[corrente];
      const contenuto = await f({ ridisegna: disegna, opzioni: opz });
      opz = {};
      if (mio === giro) corpo.replaceChildren(el('h2', {}, titolo), ...[contenuto].flat().filter(Boolean));
    } catch (e) {
      if (mio === giro) corpo.replaceChildren(el('h2', {}, titolo), el('p.errore-modulo', {}, testoErrore(e)));
    }
  };
  const togli = bus.su('impostazioni', () => { if (corrente === 'generali') disegna(); });
  const togli2 = bus.su('assistente:aggiornato', () => { if (corrente === 'squadra') disegna(); });
  apriSovrapposta([colonna, corpo], { classe: 'impostazioni', etichetta: 'Impostazioni', quandoChiusa: () => { togli(); togli2(); } });
  disegna();
}

const salva = (chiave) => (v) => scriviImpostazione(chiave, v).then(() => avvisa('Salvato')).catch((e) => avvisa(testoErrore(e)));

async function generali(esci, ridisegna) {
  const nome = stato.io?.nome || 'Utente';
  const disp = stato.io?.accesso?.dispositivo;
  const [voci, mic, fidati] = await Promise.all([api.voci().then((r) => r?.voci || r || []).catch(() => []), microfoni(),
    api.dispositiviFidati().then((r) => r?.dispositivi || []).catch((e) => e)]);
  const notifiche = statoNotifiche();
  return [
    el('div.riquadro-titolo', {}, 'Account'),
    el('div.riquadro', {}, el('div.coppia', {},
      el('span', { style: 'display:flex;align-items:center;gap:10px;color:var(--testo)' }, el('span.iniziale', {}, nome.slice(0, 1).toUpperCase()),
        el('span', {}, el('strong', {}, nome), el('small', {}, disp ? `Questo dispositivo: ${disp}` : `Versione ${stato.io?.versione || ''}`))),
      el('button.tasto', { type: 'button', onclick: () => esci?.() }, 'Esci'))),
    el('div.riquadro-titolo', {}, 'Dispositivi fidati'),
    riquadroFidati(fidati, ridisegna),
    el('div.riquadro-titolo', {}, 'Aspetto'),
    el('div.riquadro', {},
      coppia('Tema', tendina('Tema', [['chiaro', 'Chiaro'], ['scuro', 'Scuro'], ['sistema', 'Sistema']], impostazione(CHIAVI.tema, 'sistema'), salva(CHIAVI.tema))),
      coppia('Accento', tendina('Accento', [['assistente', 'Colore dell\'assistente'], ['neutro', 'Neutro']], impostazione(CHIAVI.accento, 'assistente'), salva(CHIAVI.accento))),
      coppia('Lingua', tendina('Lingua', [['it', 'Italiano']], 'it', salva(CHIAVI.lingua))),
      coppia('Pannello laterale', interruttore('Pannello laterale', impostazione(CHIAVI.pannello, true) !== false, salva(CHIAVI.pannello)), 'Aperto all\'avvio, alla misura larga')),
    el('div.riquadro-titolo', {}, 'Scrittura e chat'),
    el('div.riquadro', {},
      coppia('Controlla l\'ortografia mentre scrivo', interruttore('Controlla l\'ortografia mentre scrivo', impostazione(CHIAVI.ortografia, true) !== false, salva(CHIAVI.ortografia)),
        'Barra di scrittura e testi lunghi; comandi, indirizzi e token mai'),
      coppia('Flusso degli strumenti', tendina('Flusso degli strumenti', [['completo', 'Completo'], ['minimo', 'Minimo']], impostazione(CHIAVI.flusso, 'completo'), salva(CHIAVI.flusso)),
        'Minimo: una riga «3 strumenti · 2 s» per gruppo, si apre con un tocco')),
    el('div.riquadro-titolo', {}, 'Sistema'),
    el('div.riquadro', {},
      coppia('Notifiche', notifiche.attive ? el('span', {}, 'attive su questo dispositivo')
        : el('button.tasto.piccolo', { type: 'button', disabled: !notifiche.possibili, onclick: async () => { await chiediNotifiche({ spiega: false }); } }, notifiche.possibili ? 'Attiva' : 'non disponibili'),
      notifiche.negate ? 'Negate nel browser: riattivale dalle impostazioni del sito' : 'Un avviso quando tocca a te o c\'è una richiesta'),
      coppia('Suono notifiche', interruttore('Suono notifiche', impostazione(CHIAVI.suono, true) !== false, salva(CHIAVI.suono))),
      coppia('Microfono', tendina('Microfono', [['', 'Predefinito'], ...mic.map((d, i) => [d.deviceId, d.label || `Microfono ${i + 1}`])], impostazione(CHIAVE_MICROFONO, ''), salva(CHIAVE_MICROFONO)),
        mic.some((d) => d.label) ? null : 'I nomi compaiono dopo il primo dettato')),
    el('div.riquadro-titolo', {}, 'Voce'),
    el('div.riquadro', {},
      coppia('Risposta parlata', interruttore('Risposta parlata', impostazione(CHIAVE_PARLATA, false) === true, salva(CHIAVE_PARLATA)), 'A fine turno legge la risposta'),
      coppia('Voce di risposta', tendina('Voce di risposta', [['', 'Quella dell\'assistente'], ...voci.map((v) => [v.id, v.nome])], impostazione('interfaccia.voce', ''), salva('interfaccia.voce'))),
      coppia('Velocità', tendina('Velocità della voce', [['0.8', '0,8×'], ['0.9', '0,9×'], ['1', '1×'], ['1.1', '1,1×'], ['1.25', '1,25×'], ['1.5', '1,5×']],
        String(impostazione('interfaccia.voce_velocita', 1)), (v) => salva('interfaccia.voce_velocita')(Number(v))))),
    el('div.riquadro-titolo', {}, 'Assistenti'),
    el('div.riquadro', {},
      coppia('Fuso orario', el('span', {}, Intl.DateTimeFormat().resolvedOptions().timeZone || 'Europe/Rome'))),
  ];
}

/** Dispositivi che entrano con la sola password (finché non li revochi); revoca singola o di tutti, con conferma nel foglio. */
function riquadroFidati(fidati, ridisegna) {
  if (!Array.isArray(fidati)) return el('div.riquadro', {}, el('p.errore-modulo', {}, testoErrore(fidati)));
  const revoca = async (titolo, testo, f) => {
    if (!await chiediConferma({ titolo, testo, conferma: 'Revoca', pericolo: true })) return;
    try { await f(); avvisa('Revocato'); ridisegna(); } catch (e) { avvisa(testoErrore(e)); }
  };
  const righe = fidati.map((d) => el('div.coppia.fidato', { 'data-id': d.id },
    el('span', {}, el('strong', {}, d.nome || 'Dispositivo', d.corrente ? el('span.etichetta-corrente', {}, ' · questo dispositivo') : null),
      el('small', {}, `creato ${quandoBreve(d.creato_ms)} · ultimo uso ${d.ultimo_uso_ms ? quandoBreve(d.ultimo_uso_ms) : 'mai'} · scade ${fraQuanto(d.scade_ms)}`)),
    el('button.tasto.piccolo', { type: 'button', onclick: () => revoca(`Revocare «${d.nome || 'Dispositivo'}»?`,
      d.corrente ? 'Al prossimo accesso da qui servirà di nuovo il codice TOTP.' : 'Da quel dispositivo servirà di nuovo il codice TOTP.',
      () => api.revocaFidato(d.id)) }, 'Revoca')));
  return el('div.riquadro', {},
    righe.length ? righe : el('p.nota', {}, 'Nessun dispositivo fidato: ogni accesso chiede il codice.'),
    righe.length ? el('div.coppia', {}, el('span', {}, 'Tutti', el('small', {}, 'Ogni dispositivo chiederà di nuovo il codice')),
      el('button.tasto.piccolo.pericolo', { type: 'button', onclick: () => revoca('Revocare tutti i dispositivi fidati?',
        `Sono ${righe.length}. Ogni accesso chiederà di nuovo il codice TOTP.`, () => api.revocaFidatiTutti()) }, 'Revoca tutti')) : null);
}

export { anelloGrande } from './pagine/modelli.js';
