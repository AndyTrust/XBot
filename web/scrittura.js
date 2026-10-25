// La barra di scrittura (Layout sez. 1.3): ＋ (allega, foto, cassaforte), campo che cresce con
// `@` completato, Invio / Maiusc+Invio, bozza per sessione, dettato (tieni premuto o ⌘D), mani
// libere, risposta parlata; riga con il bottone «macchina · provider · modello» che apre il foglio
// di scelta (scelta.js), la tendina «Pensiero», quota, chip della macchina con stato («Continua su…»).

import { aggiungi, inCaricamento, preparaAllegati, pronti, ridisegnaAllegati, svuotaAllegati } from './allegati.js';
import { api, elenco, testoErrore } from './api.js';
import { etichettaMacchina, gruppiScelta, inLinea as macchinaInLinea, leggiMacchine, leggiScelta, macchinaDa, macchinaServer, macchine, STATI_MACCHINA, valoreScelta } from './macchine.js';
import { bottoneScelta, tendinaPensiero } from './scelta.js';
import { apriFigliaDa } from './figlie.js';
import { invia } from './flusso.js';
import { riconosciParallelo } from './parallelo.js';
import { anello, icona, iconaIn } from './icone.js';
import { rigaDa } from './chat.js';
import { apriManiLibere } from './manilibere.js';
import { apriMenu } from './menu.js';
import { agganciaMenzioni } from './menzioni.js';
import { assistenteDi, bus, caricaQuote, chiaveBozza, nomeProvider, providerAuto, quotaDi, scriviImpostazione, stato } from './stato.js';
import { $, avvisa, el, fraQuanto, locale, numero } from './util.js';
import { apriMicrofono, CHIAVE_PARLATA, registratore, rispostaParlata, trascrivi, zitto } from './voce.js';

let campo;
let rispostaA = null;
let scegliFile;
let scegliFoto;

export function preparaScrittura({ apriUtilizzo, apriCassaforte, apriComputer }) {
  campo = $('#campo');
  iconaIn($('#tasto-allega'), 'piu');
  iconaIn($('#tasto-microfono'), 'microfono');
  iconaIn($('#tasto-manilibere'), 'onda');
  iconaIn($('#tasto-invia'), 'invia');
  scegliFile = el('input', { type: 'file', multiple: true, hidden: true, onchange: (ev) => { aggiungi(ev.target.files); ev.target.value = ''; } });
  scegliFoto = el('input', { type: 'file', accept: 'image/*', capture: 'environment', hidden: true, onchange: (ev) => { aggiungi(ev.target.files); ev.target.value = ''; } });
  $('#scrittura').append(scegliFile, scegliFoto);
  preparaAllegati($('#allegati-scrittura'), () => aggiornaTasti());
  $('#tasto-allega').addEventListener('click', (ev) => apriMenu(ev.currentTarget, [
    { testo: 'Allega file', icona: 'graffetta', azione: () => scegliFile.click() },
    { testo: 'Foto o fotocamera', icona: 'immagine', azione: () => scegliFoto.click() },
    { testo: 'Riferimento dalla cassaforte', icona: 'chiave', azione: () => sceltaCassaforte(ev.target) },
    { testo: 'Apri la cassaforte', icona: 'lucchetto', azione: () => apriCassaforte?.() },
  ]));
  campo.addEventListener('paste', (ev) => {
    const files = [...(ev.clipboardData?.files || [])];
    if (files.length) { ev.preventDefault(); aggiungi(files); }
  });
  agganciaMenzioni(campo, $('#scrittura .scrittura-riquadro'));
  preparaDettato();
  $('#tasto-manilibere').addEventListener('click', () => apriManiLibere());
  campo.addEventListener('input', () => { cresci(); salvaBozza(); aggiornaTasti(); });
  campo.addEventListener('keydown', (ev) => {
    if (ev.key === 'Enter' && !ev.shiftKey && !ev.isComposing) { ev.preventDefault(); manda(); }
    if (ev.key === 'Escape' && rispostaA) { rispostaA = null; mostraRisposta(); }
  });
  $('#scrittura').addEventListener('submit', (ev) => { ev.preventDefault(); manda(); });
  bus.su('quote', () => disegnaRigaProvider());
  bus.su('flusso:stato', (st) => { inLinea = st === 'connesso'; disegnaRete(); });
  addEventListener('offline', () => { inLinea = false; disegnaRete(); });
  addEventListener('online', () => disegnaRete());
  bus.su('provider', () => disegnaRigaProvider());
  bus.su('assistente:aggiornato', () => { caricaScritturaSoloRiga(); });
  bus.su('sessione', () => disegnaRigaProvider());
  bus.su('rispondi', (r) => { rispostaA = r; mostraRisposta(); campo.focus(); });
  bus.su('invio:rifiutato', ({ voce }) => {
    // il testo rifiutato torna nel campo, se è vuoto
    if (voce.sessione_id === stato.sessione?.id && !campo.value) { campo.value = voce.testo; cresci(); salvaBozza(); aggiornaTasti(); }
  });
  apriUtilizzoFn = apriUtilizzo;
  apriComputerFn = apriComputer || (() => {});
  bus.su('macchine', () => disegnaRigaProvider());
  // lo stato delle macchine cambia da solo (Mac spento o riacceso): si rilegge ogni 60 s se la pagina è visibile
  setInterval(() => { if (document.visibilityState === 'visible' && stato.sessione) leggiMacchine(); }, 60000);
}

let apriUtilizzoFn = () => {};
let apriComputerFn = () => {};
let inLinea = true;

/** «senza rete» nella riga del provider quando il flusso è staccato o il browser è offline. */
function disegnaRete() {
  const riga = $('#riga-provider');
  let n = $('#senza-rete');
  const fuori = !inLinea || !navigator.onLine;
  if (fuori && !n) { n = el('span.senza-rete', { id: 'senza-rete', role: 'status', title: 'Il flusso è staccato: riprovo da solo' }, 'senza rete'); riga.prepend(n); }
  if (!fuori && n) n.remove();
}

/** Prepara il campo per la sessione aperta: segnaposto, bozza, riga del provider. */
export function caricaScrittura() {
  const a = assistenteDi(stato.assistenteId);
  campo.placeholder = `Scrivi a ${a.nome || '…'}`;
  campo.value = stato.sessione ? locale.leggi(chiaveBozza(stato.sessione.id), '') : '';
  rispostaA = null;
  ridisegnaAllegati();
  mostraRisposta();
  cresci();
  aggiornaTasti();
  disegnaRigaProvider();
}

function caricaScritturaSoloRiga() {
  if (!stato.assistenteId) return;
  campo.placeholder = `Scrivi a ${assistenteDi(stato.assistenteId).nome || '…'}`;
  caricaMacchine();
}

export function mettiFuoco() { campo?.focus(); }

function cresci() {
  campo.style.height = 'auto';
  campo.style.height = `${Math.min(campo.scrollHeight, innerHeight * 0.4)}px`;
}

function salvaBozza() {
  if (!stato.sessione) return;
  locale.scrivi(chiaveBozza(stato.sessione.id), campo.value);
  locale.scrivi(`ultima_sessione.${stato.assistenteId}`, stato.sessione.id);
  bus.emetti('bozza');
}

function aggiornaTasti() {
  const pieno = campo.value.trim().length > 0 || pronti().length > 0;
  $('#tasto-invia').hidden = !pieno;
  $('#tasto-manilibere').hidden = pieno;
}

function mostraRisposta() {
  const vecchia = $('#scrittura .citazione');
  vecchia?.remove();
  if (!rispostaA) return;
  const t = rispostaA.corpo?.testo || '';
  $('#scrittura').prepend(el('div.citazione', { style: 'padding:0 12px 6px' }, icona('rispondi', 14),
    el('span', {}, `Rispondi a «${t.slice(0, 60)}${t.length > 60 ? '…' : ''}»`),
    el('button.tasto-icona.piccolo', { type: 'button', 'aria-label': 'Annulla la risposta', onclick: () => { rispostaA = null; mostraRisposta(); } }, icona('chiudi', 12))));
}

function manda() {
  const testo = campo.value.trim();
  const allegati = pronti();
  if ((!testo && !allegati.length) || !stato.sessione) return;
  if (inCaricamento()) { avvisa('Aspetta: sto caricando gli allegati'); return; }
  // «in parallelo …»: una chat figlia con il resto del testo come compito (piano sessioni annidate)
  const par = allegati.length ? null : riconosciParallelo(testo);
  if (par) {
    apriFigliaDa(stato.sessione.id, par.compito, par).catch((e) => avvisa(testoErrore(e)));
    campo.value = '';
    rispostaA = null;
    mostraRisposta();
    salvaBozza();
    return;
  }
  invia(stato.sessione.id, testo, { risposta_a: rispostaA && rigaDa(rispostaA.id) ? rispostaA.id : undefined, allegati });
  svuotaAllegati();
  campo.value = '';
  rispostaA = null;
  mostraRisposta();
  salvaBozza();
  cresci();
  aggiornaTasti();
}

// ---------- Riga del provider ----------

/** L'anello della quota come tasto: tocco = Utilizzo. */
export function tastoQuota(providerId, { lungo = true } = {}) {
  const q = quotaDi(providerId);
  const usata = q?.percentuale_usata;
  const bassa = usata !== null && usata !== undefined && 100 - usata < 10;
  const perc = usata === null || usata === undefined ? 'quota —' : `${numero(usata)} %`;
  const azzera = usata !== null && usata !== undefined && lungo && q.azzera_ms ? ` · si azzera ${fraQuanto(q.azzera_ms)}` : '';
  const testo = perc + azzera;
  return el(`button.quota${bassa ? '.bassa' : ''}`, {
    type: 'button', title: q?.fonte === 'stima' ? 'Quota stimata dai turni' : 'Quota usata (dal client)',
    'aria-label': `Quota ${nomeProvider(providerId)}: ${testo}`, onclick: () => apriUtilizzoFn(),
  }, anello(usata, 14), el('span', {}, perc, azzera ? el('span.azzera', {}, azzera) : null));
}

export async function disegnaRigaProvider() {
  const riga = $('#riga-provider');
  const s = stato.sessione;
  if (!s) { riga.replaceChildren(); return; }
  const auto = providerAuto();
  const providerId = s.provider_id || assistenteDi(stato.assistenteId).provider_predefinito_id || auto;
  const sa = await sceltaAttuale(s);
  if (stato.sessione !== s) return; // la sessione è cambiata intanto
  const sel = el('span.scelta', {}, bottoneScelta({ testo: sa.testo, valore: sa.valore, quandoSceglie: (v) => scegli(v) }));
  const pensiero = await pensieroSessione(s, sa);
  riga.replaceChildren(
    sel,
    pensiero,
    el('span.sep', {}, '·'),
    tastoQuota(providerId),
    el('span.sep', {}, '·'),
    chipMacchina(s),
    el('span.sep', {}, '·'),
    tastoParlata(),
  );
  disegnaRete();
}

/**
 * La scelta attuale della sessione: {valore, testo, voce} (voce = null se oggi non è fra le scelte,
 * es. Mac spento: il testo resta visibile). Il percorso del programma non si mostra mai.
 */
export async function sceltaAttuale(s) {
  const gruppi = await gruppiScelta();
  const attuale = macchinaDellaSessione(s);
  const server = macchinaServer();
  const valore = valoreScelta(attuale?.id ?? server?.id ?? '', s?.provider_id || '', s?.modello_id || '');
  const voce = gruppi.flatMap((g) => g.voci).find((v) => v.valore === valore) || null;
  if (voce) return { valore, testo: voce.testo, voce };
  const m = attuale ? etichettaMacchina(attuale) : 'VPS';
  const testo = [m, s?.provider_id ? nomeProvider(s.provider_id) : 'Auto', s?.modello_id || (s?.provider_id ? 'predefinito' : null)].filter(Boolean).join(' · ');
  return { valore, testo: `${testo}${attuale && !macchinaInLinea(attuale) ? ' (spenta)' : ''}`, voce: null };
}

/** Il bottone «Mac · Cursor · Grok 4.7 Low» che apre il foglio di scelta (scelta.js). */
export async function sceltaGruppata(s, { classe = 'span.scelta' } = {}) {
  const { valore, testo } = await sceltaAttuale(s);
  return el(classe, {}, bottoneScelta({ testo, valore, quandoSceglie: (v) => scegli(v) }));
}

/** La tendina «Pensiero» della sessione, accanto al modello; null se il modello non ha livelli. */
export async function pensieroSessione(s, sa) {
  const { voce } = sa || await sceltaAttuale(s);
  return tendinaPensiero(voce?.livelli || [], s?.livello_ragionamento || '', (v) => cambiaSessione({ livello_ragionamento: v }));
}

async function scegli(voce) {
  const s = stato.sessione;
  if (!s) return;
  const v = leggiScelta(voce.valore);
  const prima = macchinaDellaSessione(s)?.id ?? macchinaServer()?.id ?? '';
  if (v.macchina_id && v.macchina_id !== prima) {
    const ok = await continuaSu(v.macchina_id, { zitto: true });
    if (!ok) return;
  }
  const campi = { provider_id: v.provider_id, modello_id: v.modello_id };
  // il livello scelto prima resta solo se il modello nuovo lo conosce
  if (stato.sessione.livello_ragionamento && !(voce.livelli || []).includes(stato.sessione.livello_ragionamento)) campi.livello_ragionamento = null;
  await cambiaSessione(campi);
}

// ---------- Macchina: chip con stato e «Continua su…» ----------
export async function caricaMacchine() {
  await leggiMacchine();
  disegnaRigaProvider();
}
export const macchinaDellaSessione = (s) => macchinaDa(s?.macchina_id) || macchinaServer() || macchine.find((m) => ['vps', 'mac'].includes(m.genere) && macchinaInLinea(m)) || null;

function chipMacchina(s) {
  const m = macchinaDellaSessione(s);
  const su = !m || macchinaInLinea(m);
  const testo = m ? etichettaMacchina(m) : (s.macchina_id || 'server');
  return el(`button.chip-macchina${su ? '' : '.spenta'}`, {
    type: 'button', title: m ? `${m.nome}: ${STATI_MACCHINA[m.stato] || m.stato}` : 'Macchina della sessione',
    'aria-label': `Macchina della sessione: ${testo}, ${m ? (STATI_MACCHINA[m.stato] || m.stato).toLowerCase() : 'server'}. Cambia`,
    onclick: (ev) => menuMacchine(ev.currentTarget),
  }, el(`span.punto-${su ? 'verde' : 'grigio'}`, { 'aria-hidden': 'true' }), el('span', {}, testo), su ? null : el('small', {}, (STATI_MACCHINA[m.stato] || m.stato).toLowerCase()));
}

function menuMacchine(ancora) {
  const attuale = macchinaDellaSessione(stato.sessione);
  apriMenu(ancora, [
    { titolo: 'Continua su' },
    ...macchine.map((m) => ({
      testo: etichettaMacchina(m), nota: STATI_MACCHINA[m.stato] || m.stato, icona: m.id === attuale?.id ? 'spunta' : 'computer',
      disattivo: !macchinaInLinea(m) || m.id === attuale?.id, azione: () => continuaSu(m.id),
    })),
    '-',
    { testo: 'Computer collegati…', icona: 'impostazioni', azione: () => apriComputerFn() },
  ]);
}

export async function continuaSu(macchinaId, { zitto: muto = false } = {}) {
  if (!stato.sessione || macchinaId === macchinaDellaSessione(stato.sessione)?.id) return true;
  let m = macchinaDa(macchinaId);
  if (!m) { await leggiMacchine(); m = macchinaDa(macchinaId); }
  try {
    const nuova = await api.sposta(stato.sessione.id, macchinaId);
    const s = nuova?.sessione || nuova;
    stato.sessione = s?.id ? s : { ...stato.sessione, macchina_id: macchinaId };
    if (!muto) avvisa(`Continua su ${m ? etichettaMacchina(m) : 'l\'altra macchina'}`);
    bus.emetti('sessione');
    disegnaRigaProvider();
    return true;
  } catch (e) { avvisa(testoErrore(e)); disegnaRigaProvider(); return false; }
}

// ---------- Risposta parlata ----------
function tastoParlata() {
  const su = rispostaParlata();
  return el(`button.quota.parlata${su ? '.accesa' : ''}`, {
    type: 'button', 'aria-pressed': su ? 'true' : 'false', title: su ? 'Risposta parlata: accesa' : 'Risposta parlata: spenta',
    'aria-label': 'Risposta parlata', onclick: async () => {
      if (su) zitto();
      try { await scriviImpostazione(CHIAVE_PARLATA, !su); } catch (e) { avvisa(testoErrore(e)); }
      avvisa(!su ? 'Risposta parlata accesa' : 'Risposta parlata spenta');
      disegnaRigaProvider();
    },
  }, icona('altoparlante', 14), el('span', {}, su ? 'voce' : 'muto'));
}

// ---------- Riferimento dalla cassaforte (solo il titolo, mai il valore) ----------
async function sceltaCassaforte(ancora) {
  let voci = [];
  try { voci = elenco(await api.cassaforte(), 'voci'); } catch (e) { avvisa(testoErrore(e)); return; }
  if (!voci.length) { avvisa('La cassaforte è vuota'); return; }
  apriMenu(ancora, [{ titolo: 'Inserisci il riferimento' }, ...voci.slice(0, 14).map((v) => ({
    testo: v.titolo, nota: v.genere, icona: 'chiave',
    azione: () => { inserisci(`«cassaforte: ${v.titolo}» `); },
  }))]);
}

function inserisci(t) {
  const i = campo.selectionStart ?? campo.value.length;
  campo.value = campo.value.slice(0, i) + t + campo.value.slice(i);
  campo.setSelectionRange(i + t.length, i + t.length);
  campo.dispatchEvent(new Event('input', { bubbles: true }));
  campo.focus();
}

// ---------- Dettato: tieni premuto il microfono (o tocca per avviare e fermare), ⌘D ----------
let dettato = null; // {rec, flusso, tenuto}

function preparaDettato() {
  const t = $('#tasto-microfono');
  let premuto = 0;
  t.addEventListener('pointerdown', (ev) => { ev.preventDefault(); premuto = performance.now(); if (!dettato) avviaDettato(); });
  t.addEventListener('pointerup', () => { if (dettato && performance.now() - premuto > 450) fermaDettato(); else if (dettato && dettato.secondoTocco) fermaDettato(); else if (dettato) dettato.secondoTocco = true; });
  t.addEventListener('contextmenu', (ev) => ev.preventDefault());
  t.addEventListener('keydown', (ev) => { if (ev.key === 'Enter' || ev.key === ' ') { ev.preventDefault(); if (dettato) fermaDettato(); else avviaDettato(); } });
  document.addEventListener('keydown', (ev) => {
    if ((ev.metaKey || ev.ctrlKey) && ev.key.toLowerCase() === 'd' && !$('#finestra').hidden && stato.sessione) {
      ev.preventDefault();
      if (dettato) fermaDettato(); else avviaDettato();
    }
  });
}

async function avviaDettato() {
  const t = $('#tasto-microfono');
  dettato = { avvio: true };
  try {
    const flusso = await apriMicrofono();
    if (!dettato) { flusso.getTracks().forEach((x) => x.stop()); return; }
    const rec = registratore(flusso);
    rec.avvia();
    Object.assign(dettato, { rec, flusso, avvio: false });
    t.classList.add('registra');
    t.setAttribute('aria-label', 'Ferma il dettato');
    avvisa('Parla: rilascia o tocca di nuovo per finire', 2000);
  } catch (e) {
    dettato = null;
    avvisa(e.message || String(e), 5000);
  }
}

async function fermaDettato() {
  const d = dettato;
  dettato = null;
  const t = $('#tasto-microfono');
  t.classList.remove('registra');
  t.setAttribute('aria-label', 'Dettato: tieni premuto e parla (⌘D)');
  if (!d?.rec) return;
  const audio = await d.rec.ferma();
  d.flusso.getTracks().forEach((x) => x.stop());
  if (audio.size < 800) { avvisa('Troppo breve: tieni premuto mentre parli'); return; }
  t.classList.add('trascrive');
  try {
    const testo = await trascrivi(audio);
    if (testo) inserisci((campo.value && !/\s$/.test(campo.value) ? ' ' : '') + testo); else avvisa('Non ho sentito parole');
  } catch (e) { avvisa(`Trascrizione non riuscita: ${testoErrore(e)}`, 4000); }
  t.classList.remove('trascrive');
}

export async function cambiaSessione(campi) {
  try {
    stato.sessione = await api.modificaSessione(stato.sessione.id, campi);
    bus.emetti('sessione');
    disegnaRigaProvider();
    caricaQuote();
  } catch (e) {
    avvisa(testoErrore(e));
    disegnaRigaProvider();
  }
}
