// La colonna della chat (Layout sez. 1.2): separatori di tempo, bolle con Markdown, righe
// strumento richiudibili, avvisi, ragionamento, bolle «in invio» dal diario, stato del turno.

import { api, testoErrore } from './api.js';
import { avatar, avatarVivo, coloreDi } from './avatar.js';
import { inAttesa, invia } from './flusso.js';
import { icona } from './icone.js';
import { attivaCopia, markdown } from './markdown.js';
import { apriMenu } from './menu.js';
import { rigaFiglia } from './figlie.js';
import { richiesteAperte, schedaRichiesta } from './richieste.js';
import { assistenteDi, bus, CHIAVI, impostazione, nomeProvider, stato } from './stato.js';
import { $, avvisa, copia, dimensione, el, quandoBreve, testoDa } from './util.js';
import { parla } from './voce.js';

const DIECI_MINUTI = 10 * 60 * 1000;
const righe = new Map(); // id → riga
let ordine = [];         // righe in ordine di seq
let nodi = new Map();    // id → nodo
let contenitore;
let gruppoInvio;
let gruppoRichieste;     // richieste aperte della sessione che non hanno ancora una riga
let tastoNuovi;
let agganciato = true;           // la vista segue il fondo finché l'utente non sale
const inCoda = new Set();        // righe eco degli invii in coda al turno in corso

export function preparaChat() {
  contenitore = $('#messaggi');
  gruppoInvio = el('div.in-invio-gruppo');
  gruppoRichieste = el('div.richieste-gruppo');
  bus.testoDelTurno = (turnoId) => {
    const r = [...righe.values()].filter((x) => x.genere === 'messaggio' && x.ruolo === 'assistente' && x.corpo?.turno_id === turnoId).pop();
    return r?.corpo?.testo || '';
  };
  bus.su('richieste', () => caricaRichieste());
  bus.su('flusso:richiesta', ({ sessioneId }) => { if (sessioneId === stato.sessione?.id) caricaRichieste(); });
  bus.su('flusso:turno', () => caricaRichieste());
  // Il flusso SSE non ha un frame «richiesta»: si rileggono le richieste ai frame turno e rows,
  // quando la barra accende «in attesa di te» per l'assistente aperto, e ogni 60 s finché resta acceso.
  let inAttesaPrima = false;
  let ultimaLettura = 0;
  bus.su('barra', () => {
    const ora = assistenteDi(stato.assistenteId).in_attesa_utente ? true : false;
    if ((ora && !inAttesaPrima) || (ora && Date.now() - ultimaLettura > 60000)) { ultimaLettura = Date.now(); caricaRichieste(); }
    inAttesaPrima = ora;
  });
  attivaCopia(contenitore, avvisa);
  tastoNuovi = el('button.nuovi-messaggi', { type: 'button', hidden: true, onclick: () => { agganciato = true; inFondo(); } }, '↓ nuovi messaggi');
  $('#chat').append(tastoNuovi);
  // Si stacca solo se è l'utente a salire (rotella, dito, tasti); le crescite del contenuto
  // e i cambi d'altezza della colonna riportano in fondo finché si è agganciati.
  let ultimoTop = 0;
  contenitore.addEventListener('scroll', () => {
    const salito = contenitore.scrollTop < ultimoTop - 2;
    ultimoTop = contenitore.scrollTop;
    if (vicinoAlFondo()) { agganciato = true; tastoNuovi.hidden = true; } else if (salito) agganciato = false;
  }, { passive: true });
  const segui = () => { if (agganciato && !vicinoAlFondo()) contenitore.scrollTop = contenitore.scrollHeight; };
  new ResizeObserver(segui).observe(contenitore);
  new MutationObserver(segui).observe(contenitore, { childList: true, subtree: true, characterData: true });
  bus.su('invio:esito', ({ esito }) => {
    if (esito?.consegna === 'in_coda_prossimo_turno' && esito.riga_eco_id) { inCoda.add(esito.riga_eco_id); ridisegna(esito.riga_eco_id); }
  });
  contenitore.addEventListener('contextmenu', menuBolla);
  bus.su('flusso:righe', ({ sessioneId, righe: nuove }) => { if (sessioneId === stato.sessione?.id) applica(nuove); });
  bus.su('flusso:azzera', ({ sessioneId }) => { if (sessioneId === stato.sessione?.id) svuota(); });
  bus.su('diario', () => mostraInInvio());
  bus.su('invio:esito', () => mostraInInvio());
  bus.su('flusso:turno', ({ sessioneId, turno_id, stato: st }) => {
    if (sessioneId !== stato.sessione?.id) return;
    stato.turno = st === 'in_corso' ? { turno_id, stato: st } : null;
    if (st === 'in_corso' && inCoda.size) { const v = [...inCoda]; inCoda.clear(); v.forEach(ridisegna); }
    mostraTurno();
  });
  bus.su('barra', () => segnaLetto());
  bus.su('chiamata:finita', async (x) => {
    // la riga «Chat vocale» la scrive il server (CONTRATTO-2E §12) e arriva dal flusso: si vede da ogni dispositivo
    agganciato = true;
    try { await api.salvaChiamata(x.sessioneId, { iniziata_ms: x.iniziata_ms, durata_ms: x.durata_ms, invii: x.invii, risposte: x.risposte }); } catch (e) { avvisa(`Chiamata non salvata: ${testoErrore(e)}`); }
  });
  // le chiamate e i voti tenuti nel browser fino alla 2.E (ora sul server) si tolgono
  try { for (const k of Object.keys(localStorage)) if (/(^|\.)(chiamate\.|chiamata\.voto\.)/.test(k)) localStorage.removeItem(k); } catch { /* niente archivio */ }
  // fine turno: la bolla perde il cursore e mostra le righe «Messaggio a …»
  bus.su('flusso:turno', ({ sessioneId, turno_id, stato: st }) => {
    if (sessioneId !== stato.sessione?.id || st === 'in_corso') return;
    for (const r of righe.values()) if (r.genere === 'messaggio' && r.ruolo === 'assistente' && r.corpo?.turno_id === turno_id) ridisegna(r.id);
    raggruppaColleghi();
  });
  bus.su('flusso:stato', (st) => { if (st === 'connesso') setTimeout(aggiornaCursore, 300); });
  document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'visible') segnaLetto(); });
}

/** Svuota la chat (cambio di sessione, nuova generazione). */
export function svuota() {
  libreriaChiesta = null;
  nonLettiAllApertura = stato.assistenteId ? Number(assistenteDi(stato.assistenteId).non_letti || 0) : 0;
  nuoviMessi = false;
  righe.clear();
  ordine = [];
  nodi = new Map();
  inCoda.clear();
  agganciato = true;
  if (tastoNuovi) tastoNuovi.hidden = true;
  contenitore.replaceChildren(gruppoInvio, gruppoRichieste);
  gruppoRichieste.replaceChildren();
  caricaRichieste();
  aggiornaColore();
  mostraInInvio();
  setTimeout(aggiornaCursore, 400); // sessione vuota: il messaggio di benvenuto
}

export function aggiornaColore() {
  const a = stato.assistenteId ? assistenteDi(stato.assistenteId) : null;
  const colorata = impostazione(CHIAVI.accento, 'assistente') === 'assistente' && a && a.colore_in_chat !== 0;
  contenitore.classList.toggle('accento-assistente', !!colorata);
  if (a) contenitore.style.setProperty('--colore-assistente', coloreDi(a));
  for (const n of contenitore.querySelectorAll('.fila.assistente')) n.classList.toggle('colorata', !!colorata);
}

const vicinoAlFondo = () => contenitore.scrollHeight - contenitore.scrollTop - contenitore.clientHeight < 80;
function inFondo() {
  contenitore.scrollTo({ top: contenitore.scrollHeight, behavior: 'instant' });
  tastoNuovi.hidden = true;
  // dopo l'impaginazione (font, codice) il fondo può spostarsi ancora
  requestAnimationFrame(() => { if (agganciato) contenitore.scrollTop = contenitore.scrollHeight; });
}

function ridisegna(id) {
  const r = righe.get(id);
  const vecchio = nodi.get(id);
  if (!r || !vecchio) return;
  const nuovo = disegna(r, vecchio);
  vecchio.replaceWith(nuovo);
  nodi.set(id, nuovo);
}

/** Inserisce o riscrive le righe per seq. */
export function applica(nuove) {
  const attaccato = agganciato || ordine.length === 0;
  let ricostruisci = false;
  for (const r of nuove) {
    const giaVista = righe.has(r.id);
    righe.set(r.id, r);
    if (giaVista) {
      const i = ordine.findIndex((x) => x.id === r.id);
      if (i >= 0) ordine[i] = r;
      const vecchio = nodi.get(r.id);
      if (vecchio) {
        const nuovo = disegna(r, vecchio);
        vecchio.replaceWith(nuovo);
        nodi.set(r.id, nuovo);
      }
      continue;
    }
    const ultima = ordine[ordine.length - 1];
    if (ultima && r.seq < ultima.seq) { ordine.push(r); ricostruisci = true; continue; }
    ordine.push(r);
    if (!ricostruisci) aggiungiInCoda(r, ultima);
  }
  if (ricostruisci) {
    ordine.sort((a, b) => a.seq - b.seq);
    contenitore.replaceChildren();
    nodi = new Map();
    let prima = null;
    for (const r of ordine) { aggiungiInCoda(r, prima); prima = r; }
  }
  contenitore.append(gruppoInvio, gruppoRichieste);
  mettiNuovi();
  raggruppaColleghi();
  mostraInInvio();
  if (nuove.some((r) => r.genere === 'richiesta')) caricaRichieste();
  aggiornaCursore();
  if (attaccato) { agganciato = true; inFondo(); } else if (nuove.length) tastoNuovi.hidden = false;
  segnaLetto();
}

function aggiungiInCoda(r, prima) {
  if (!prima || r.creata_ms - prima.creata_ms > DIECI_MINUTI || new Date(r.creata_ms).toDateString() !== new Date(prima.creata_ms).toDateString()) {
    contenitore.append(el('div.separatore', {}, quandoBreve(r.creata_ms)));
  }
  const n = disegna(r);
  nodi.set(r.id, n);
  contenitore.append(n);
}

/** Il disegno di una riga secondo il genere. */
function disegna(r, vecchio) {
  const c = r.corpo || {};
  switch (r.genere) {
    case 'messaggio': return bolla(r, c);
    case 'chiamata_strumento':
    case 'esito': return strumento(r, c, vecchio);
    case 'ragionamento': {
      const aperto = vecchio?.querySelector('details')?.open;
      return el('div.riga-ragionamento', { dati: { id: r.id } },
        el('details', { open: aperto }, el('summary', {}, 'Ragionamento'), el('p', {}, c.testo || '')));
    }
    case 'richiesta': return el('div.fila-richiesta', { dati: { id: r.id } }, schedaRichiesta({ ...c, sessione_id: r.sessione_id, assistente_id: stato.assistenteId, creata_ms: r.creata_ms, aggiornata_ms: c.stato && c.stato !== 'aperta' ? r.aggiornata_ms : null }));
    case 'evento': if (c.tipo === 'chiamata_vocale') return rigaChiamataServer(r, c);
      if (c.codice === 'figlia') return rigaFiglia(r, c);
      return el('div.riga-avviso', { dati: { id: r.id } }, icona('info', 15), el('span', {}, c.testo || 'Evento'));
    case 'avviso': if (inAttesaDelMac(c)) return rigaAttesa(r, c);
      return el(`div.riga-avviso${c.codice ? '.errore' : ''}`, { dati: { id: r.id } }, icona('avviso', 15), el('span', {}, c.testo || c.codice || 'Avviso'));
    default: return el('div.riga-avviso', { dati: { id: r.id } }, icona('info', 15), el('span', {}, `${r.genere.replace(/_/g, ' ')}${c.testo ? `: ${c.testo}` : ''}`));
  }
}

// ---------- «in attesa del Mac» (CONTRATTO-2E sez. 3): il turno aspetta il Mac spento ----------
const inAttesaDelMac = (c) => c.codice === 'in_attesa_macchina' || c.codice === 'lavoro_in_attesa' || /in attesa del mac/i.test(c.testo || '');

function rigaAttesa(r, c) {
  const chiuso = c.risolto || c.stato === 'partito' || c.stato === 'scaduto';
  const tasto = el('button.tasto.piccolo', { type: 'button', onclick: async () => {
    tasto.disabled = true;
    try {
      const esito = await api.eseguiSullaVps(c.turno_id || c.lavoro_id);
      avvisa('Lo eseguo sulla VPS con Claude');
      // la sessione passa sulla VPS con Claude Code: la riga del provider lo deve mostrare
      if (r.sessione_id === stato.sessione?.id) {
        stato.sessione = esito?.sessione?.id ? esito.sessione : await api.sessione(r.sessione_id).catch(() => stato.sessione);
        bus.emetti('sessione');
      }
      tasto.replaceWith(el('small', {}, 'spostato sulla VPS'));
    } catch (e) { tasto.disabled = false; avvisa(testoErrore(e)); }
  } }, icona('computer', 14), 'Esegui sulla VPS con Claude');
  return el('div.riga-avviso.attesa', { dati: { id: r.id }, role: 'status' }, icona('avviso', 15),
    el('span', {}, c.testo || 'In attesa del Mac: il turno parte quando il Mac si ricollega.'),
    chiuso ? el('small', {}, c.stato === 'scaduto' ? 'scaduto' : 'ripartito') : (c.turno_id || c.lavoro_id) ? tasto : null);
}

function bolla(r, c) {
  if (r.ruolo === 'collega') return bollaCollega(r, c);
  if (r.ruolo === 'utente' && /^routine:/.test(c.origine || '')) return rigaRoutine(r, c);
  if (r.ruolo === 'assistente' && !(stato.turno && c.turno_id === stato.turno.turno_id)) {
    const dest = destinatari(c.testo || '');
    if (dest.length) {
      const f = bollaNormale(r, c);
      return el('div.con-menzioni', { dati: { id: r.id } }, f, ...dest.map((d) => el('div.riga-collega', { dati: { nome: d.nome } },
        el('span.riga-collega-testo', {}, 'Messaggio a ', avatar(d, 16, { stato: false }), el('strong', {}, d.nome)))));
    }
  }
  return bollaNormale(r, c);
}

// ---------- Righe piccole di una sola linea: messaggi ai colleghi, routine, chiamate ----------
/** I colleghi nominati con `@menzione` in un testo dell'assistente (il server glielo manda). */
function destinatari(testo) {
  const visti = new Map();
  for (const m of testo.matchAll(/(^|[^\w@])@([\p{L}\p{N}_-]+)/gu)) {
    const a = [...stato.assistenti.values()].find((x) => (x.menzione || '').toLowerCase() === m[2].toLowerCase() && x.id !== stato.assistenteId && !x.archiviato_ms);
    if (a) visti.set(a.id, a);
  }
  return [...visti.values()];
}

/** Righe «Messaggio a X» consecutive con lo stesso collega diventano «N messaggi con X». */
function raggruppaColleghi() {
  const tutte = [...contenitore.querySelectorAll('.riga-collega')];
  for (const n of tutte) { n.hidden = false; n.querySelector('.conteggio')?.remove(); }
  let gruppo = [];
  const chiudi = () => {
    if (gruppo.length > 1) {
      gruppo.slice(0, -1).forEach((n) => { n.hidden = true; });
      const ultimo = gruppo[gruppo.length - 1];
      const t = ultimo.querySelector('.riga-collega-testo');
      t.firstChild.textContent = `${gruppo.length} messaggi con `;
    } else if (gruppo.length === 1) gruppo[0].querySelector('.riga-collega-testo').firstChild.textContent = 'Messaggio a ';
    gruppo = [];
  };
  for (const n of tutte) {
    const prima = gruppo[gruppo.length - 1];
    // stesso collega e nessun messaggio dell'utente in mezzo
    const inMezzo = prima && [...contenitore.querySelectorAll('.fila.utente')].some((u) => prima.compareDocumentPosition(u) & Node.DOCUMENT_POSITION_FOLLOWING && u.compareDocumentPosition(n) & Node.DOCUMENT_POSITION_FOLLOWING);
    if (prima && (prima.dataset.nome !== n.dataset.nome || inMezzo)) chiudi();
    gruppo.push(n);
  }
  chiudi();
}

const nomiRoutine = new Map(); // id → nome, letti una volta per assistente
function rigaRoutine(r, c) {
  const rid = c.origine.slice(8);
  const nome = el('strong', {}, nomiRoutine.get(rid) || 'routine');
  if (!nomiRoutine.has(rid) && stato.assistenteId) {
    nomiRoutine.set(rid, null);
    api.routine(stato.assistenteId).then((x) => {
      for (const y of (Array.isArray(x) ? x : x?.routine || [])) nomiRoutine.set(y.id, y.nome);
      if (nomiRoutine.get(rid)) nome.textContent = nomiRoutine.get(rid);
    }).catch(() => {});
  }
  return el('div.riga-piccola', { dati: { id: r.id }, title: c.testo || '' }, el('span', {}, 'Aggiornamento della routine '), icona('orologio', 14), nome);
}

function minuti(ms) { const t = Math.round((ms || 0) / 1000); return `${Math.floor(t / 60)}:${String(t % 60).padStart(2, '0')}`; }

/**
 * 👍 👎 di una riga del server (`POST /righe/{id}/voto`): lo stato viene da `corpo.voto`;
 * ritoccare il voto scelto lo toglie (`null`).
 */
function tastiVoto(r, [suTesto, giuTesto]) {
  const voto = r.corpo?.voto ?? null;
  const tasto = (v, ic, etichetta) => el(`button.tasto-icona.piccolo${voto === v ? '.scelto' : ''}`, {
    type: 'button', 'aria-label': etichetta, title: etichetta, 'aria-pressed': voto === v ? 'true' : 'false',
    onclick: async (ev) => {
      const nuovo = voto === v ? null : v;
      const box = ev.currentTarget.parentElement;
      box.querySelectorAll('button').forEach((b) => { b.disabled = true; });
      try {
        const esito = await api.vota(r.id, nuovo);
        const corpo = esito?.corpo || { ...(righe.get(r.id)?.corpo || r.corpo), voto: esito && 'voto' in esito ? esito.voto : nuovo };
        const attuale = righe.get(r.id);
        if (attuale) { righe.set(r.id, { ...attuale, ...(esito?.genere ? esito : {}), corpo }); ridisegna(r.id); }
        if (nuovo) avvisa('Grazie del voto');
      } catch (e) { box.querySelectorAll('button').forEach((b) => { b.disabled = false; }); avvisa(testoErrore(e)); }
    },
  }, icona(ic, 15));
  return el('span.voti', {}, tasto('su', 'pollicesu', suTesto), tasto('giu', 'pollicegiu', giuTesto));
}

/** La riga `evento` `{tipo: chiamata_vocale, durata_ms, voto}` scritta dal server a fine chiamata. */
function rigaChiamataServer(r, c) {
  const scambi = c.invii ? `${c.invii} ${c.invii === 1 ? 'messaggio' : 'messaggi'}, ${c.risposte ?? 0} ${c.risposte === 1 ? 'risposta' : 'risposte'}` : '';
  return el('div.riga-piccola.chiamata-finita', { dati: { id: r.id }, title: scambi }, icona('microfono', 14), el('span', {}, `Chat vocale · ${minuti(c.durata_ms)}`),
    tastiVoto(r, ['Bella chiamata', 'Chiamata da migliorare']));
}

// ---------- «NUOVI»: separatore sopra i messaggi non letti all'apertura ----------
let nonLettiAllApertura = 0;
let nuoviMessi = false;
function mettiNuovi() {
  if (nuoviMessi || !nonLettiAllApertura || !ordine.length) return;
  nuoviMessi = true;
  const loro = ordine.filter((r) => r.genere === 'messaggio' && r.ruolo !== 'utente');
  const primo = loro[Math.max(0, loro.length - nonLettiAllApertura)];
  const n = primo && nodi.get(primo.id);
  if (n) n.before(el('div.separatore-nuovi', { role: 'separator' }, el('span', {}, 'Nuovi')));
}

function bollaNormale(r, c) {
  const utente = r.ruolo === 'utente';
  const b = el('div.bolla', { dati: { id: r.id } });
  if (c.risposta_a && righe.get(c.risposta_a)) {
    const orig = righe.get(c.risposta_a).corpo?.testo || '';
    b.append(el('div.citazione', { title: 'Vai al messaggio originale', onclick: () => nodi.get(c.risposta_a)?.scrollIntoView({ block: 'center' }) },
      icona('rispondi', 14), `«${orig.slice(0, 70)}${orig.length > 70 ? '…' : ''}»`));
  }
  const corpo = el('div.md', { html: markdown(c.testo || '') });
  const coda = utente && inCoda.has(r.id) ? el('span.in-coda', {}, 'in coda') : null;
  if (!utente && stato.turno && c.turno_id === stato.turno.turno_id) corpo.classList.add('in-arrivo');
  b.append(corpo);
  if (c.allegati?.length) b.append(elencoAllegati(c.allegati));
  if (coda) b.append(coda);
  const colorata = !utente && contenitore.classList.contains('accento-assistente');
  const fila = el(`div.fila.${utente ? 'utente' : 'assistente'}${colorata ? '.colorata' : ''}`, { dati: { id: r.id } }, b);
  if (!utente && c.testo) {
    fila.append(el('div.bolla-azioni', {},
      el('button', { type: 'button', 'aria-label': 'Leggi ad alta voce', title: 'Parla', onclick: () => parla(r.corpo?.testo || '') }, icona('altoparlante', 15)),
      el('button', { type: 'button', 'aria-label': 'Copia il messaggio', title: 'Copia', onclick: () => copia(r.corpo?.testo || '') }, icona('copia', 15)),
      r.id ? tastiVoto(r, ['Risposta buona', 'Risposta da migliorare']) : null));
  }
  return fila;
}

/** Il testo di un altro assistente: cornice con il nome del mittente (mai un ordine). */
// Il nome del mittente: dal corpo se c'è, altrimenti dalla cornice «Aa ti scrive: …» del server
// o dall'origine `collega:<sessione>:<passo>` (sessione → assistente, letta una volta).
const assistenteDiSessione = new Map();
function mittenteDi(r, c) {
  if (c.mittente_id) return assistenteDi(c.mittente_id);
  if (c.mittente_nome || c.mittente) return { nome: c.mittente_nome || c.mittente };
  const cornice = /^(.{1,80}?) ti scrive:/.exec(c.testo || '');
  const daNome = cornice && [...stato.assistenti.values()].find((a) => a.nome === cornice[1]);
  if (daNome) return daNome;
  const sid = /^collega:([^:]+)/.exec(c.origine || '')?.[1];
  if (sid) {
    const aid = assistenteDiSessione.get(sid);
    if (aid) return assistenteDi(aid);
    if (aid === undefined) {
      assistenteDiSessione.set(sid, null);
      api.sessione(sid).then((s) => { assistenteDiSessione.set(sid, s.assistente_id); ridisegna(r.id); }).catch(() => {});
    }
  }
  return { nome: cornice ? cornice[1] : 'Collega' };
}

function bollaCollega(r, c) {
  const mittente = mittenteDi(r, c);
  const nome = mittente.nome || 'Collega';
  return el('div.fila.collega', { dati: { id: r.id } },
    el('div.bolla.cornice', {},
      el('div.collega-testa', {}, avatar(mittente, 18, { stato: false }), el('strong', {}, nome), el('span', {}, 'ti scrive')),
      el('div.md', { html: markdown(c.testo || '') })));
}

// Il server mette nella riga solo gli id degli allegati: nome, blob e tipo vengono dalla Libreria.
const allegatiNoti = new Map();
let libreriaChiesta = null;
export function ricordaAllegato(a) { if (a?.id) allegatiNoti.set(a.id, a); }

async function caricaLibreria() {
  const aid = stato.assistenteId;
  if (!aid || libreriaChiesta === aid) return;
  libreriaChiesta = aid;
  try {
    const r = await api.libreria(aid);
    for (const f of (Array.isArray(r) ? r : r?.file || [])) ricordaAllegato(f);
  } catch { return; }
  for (const r of righe.values()) if (r.corpo?.allegati?.some((x) => typeof x === 'string')) ridisegna(r.id);
}

/** Gli allegati dentro una bolla: anteprima delle immagini, nome e scarico. */
function elencoAllegati(allegati) {
  return el('div.allegati-bolla', {}, allegati.map((x) => {
    const a = typeof x === 'string' ? allegatiNoti.get(x) : x;
    if (!a) { caricaLibreria(); return el('span.allegato', {}, icona('file', 16), el('span', {}, 'allegato')); }
    const url = a.blob_hash ? `/api/v1/blob/${encodeURIComponent(a.blob_hash)}` : null;
    const immagine = url && /^image\//.test(a.tipo_mime || '');
    return el('a.allegato', { href: url || undefined, download: a.nome || true, target: '_blank', rel: 'noopener' },
      immagine ? el('img', { src: url, alt: a.nome || 'immagine', loading: 'lazy' }) : icona('file', 16),
      el('span', {}, a.nome || 'file'), a.dimensione ? el('small', {}, dimensione(a.dimensione)) : null);
  }));
}

/** Le richieste aperte della sessione senza una riga in chat (le mostra in fondo). */
let caricaInCorso = false;
async function caricaRichieste() {
  if (!stato.sessione || caricaInCorso) return;
  caricaInCorso = true;
  const sid = stato.sessione.id;
  try {
    const aperte = (await richiesteAperte()).filter((q) => q.sessione_id === sid);
    if (stato.sessione?.id !== sid) return;
    const conRiga = new Set([...righe.values()].filter((x) => x.genere === 'richiesta').map((x) => x.corpo?.richiesta_id));
    gruppoRichieste.replaceChildren(...aperte.filter((q) => !conRiga.has(q.id)).map((q) => el('div.fila-richiesta', {}, schedaRichiesta(q))));
  } finally { caricaInCorso = false; }
}

function riassunto(nome, input) {
  if (!input || typeof input !== 'object') return testoDa(input).split('\n')[0];
  return input.command || input.comando || input.file_path || input.path || input.pattern || input.url || input.query
    || Object.values(input).find((v) => typeof v === 'string') || '';
}

function strumento(r, c, vecchio) {
  const aperto = vecchio?.querySelector('details')?.open;
  const inCorso = c.stato === 'in_corso';
  const ok = c.ok === true || c.stato === 'riuscito';
  const esito = inCorso ? el('span.giro-piccolo', { 'aria-label': 'in corso' }, '◌')
    : el(`span.esito.${ok ? 'ok' : 'no'}`, { 'aria-label': ok ? 'riuscito' : 'fallito' }, ok ? '✓' : '✗');
  const dettagli = [];
  if (c.input !== undefined) dettagli.push(el('pre', {}, testoDa(c.input)));
  if (c.esito !== undefined) dettagli.push(el('pre', {}, testoDa(c.esito)));
  return el('div.riga-strumento', { dati: { id: r.id } },
    el('details', { open: aperto },
      el('summary', {}, icona('destra', 14, 'freccia'), el('span.nome', {}, c.nome || 'Strumento'),
        el('span.dettaglio', {}, riassunto(c.nome, c.input)), esito),
      ...dettagli));
}

/** Le bolle degli invii che il server non ha ancora confermato. */
export function mostraInInvio() {
  if (!gruppoInvio || !stato.sessione) return;
  const visti = new Set([...righe.values()].map((r) => r.corpo?.invio_id).filter(Boolean));
  const attesa = inAttesa(stato.sessione.id).filter((v) => !visti.has(v.id));
  gruppoInvio.replaceChildren(...attesa.map((v) => el('div.fila.utente', {},
    el('div.bolla.in-invio', { title: v.tentativi ? `Riprovo (tentativo ${v.tentativi + 1})` : 'In invio' }, el('div.md', { html: markdown(v.testo) })))));
  if (attesa.length) { agganciato = true; inFondo(); }
}

/** La riga di stato del turno: «<avatar> Sta scrivendo… (modello · macchina)» con Interrompi. */
export function mostraTurno() {
  const n = $('#stato-turno');
  for (const x of contenitore.querySelectorAll('.md.in-arrivo')) x.classList.remove('in-arrivo');
  if (!stato.turno || !stato.sessione) { n.hidden = true; return; }
  const a = assistenteDi(stato.assistenteId);
  const s = stato.sessione;
  const dove = [s.modello_id, s.macchina_id || 'vps'].filter(Boolean).join(' · ');
  const t = stato.turno;
  n.replaceChildren(avatarVivo({ ...a, al_lavoro: 1 }, 28), el('span', { title: `${a.nome || 'L\'assistente'} sta lavorando` }, 'Sta scrivendo…'), el('span.puntini', { 'aria-hidden': 'true' }, el('i'), el('i'), el('i')),
    dove ? el('span', { style: 'color:var(--testo-3)' }, `(${dove})`) : null,
    el('button', { type: 'button', onclick: async (ev) => {
      ev.currentTarget.disabled = true;
      try { await api.interrompi(t.turno_id); avvisa('Turno interrotto'); } catch (e) { avvisa(testoErrore(e)); }
    } }, 'Interrompi'));
  n.hidden = false;
  // la bolla dell'assistente di questo turno mostra il cursore mentre arriva
  for (const r of righe.values()) {
    if (r.genere === 'messaggio' && r.ruolo === 'assistente' && r.corpo?.turno_id === t.turno_id) {
      nodi.get(r.id)?.querySelector('.md')?.classList.add('in-arrivo');
    }
  }
}

function aggiornaCursore() {
  if (stato.turno) mostraTurno();
  const vuoto = $('#vuoto');
  if (!stato.sessione) return;
  vuoto.hidden = ordine.length > 0 || inAttesa(stato.sessione?.id).length > 0;
}

let timerLetto;
let ultimoLetto = '';
/** Segna letto quando la chat è aperta e la pagina visibile. */
export function segnaLetto() {
  if (!stato.assistenteId || document.visibilityState !== 'visible') return;
  const a = assistenteDi(stato.assistenteId);
  if (!a.non_letti && !a.in_attesa_utente) return;
  const id = stato.assistenteId;
  const fino = ordine.length ? ordine[ordine.length - 1].seq : 0;
  // una volta sola per lo stesso punto: «in attesa» può restare acceso per una richiesta aperta
  if (ultimoLetto === `${id}:${fino}`) return; // una volta per punto: evita il giro letto → barra → letto
  ultimoLetto = `${id}:${fino}`;
  clearTimeout(timerLetto);
  timerLetto = setTimeout(async () => {
    try { await api.letto(id, fino); bus.emetti('letto', id); } catch { /* al prossimo giro */ }
  }, 400);
}

function menuBolla(ev) {
  const fila = ev.target.closest('.fila');
  if (!fila || fila.querySelector('.in-invio')) return;
  const r = righe.get(fila.dataset.id);
  if (!r) return;
  ev.preventDefault();
  const domanda = r.ruolo === 'assistente' ? [...righe.values()].filter((x) => x.seq < r.seq && x.genere === 'messaggio' && x.ruolo === 'utente').pop() : r;
  const altri = stato.provider.filter((p) => p.attivo && p.caricato !== false && p.id !== (stato.sessione?.provider_id || null));
  apriMenu({ x: ev.clientX, y: ev.clientY }, [
    { testo: 'Copia', icona: 'copia', azione: () => copia(r.corpo?.testo || '') },
    { testo: 'Rispondi', icona: 'rispondi', azione: () => bus.emetti('rispondi', r) },
    r.corpo?.testo ? { testo: 'Leggi ad alta voce', icona: 'altoparlante', azione: () => parla(r.corpo.testo) } : null,
    ...(domanda?.corpo?.testo && altri.length ? ['-', { titolo: 'Riprova con' }, ...altri.map((p) => ({
      testo: nomeProvider(p.id), icona: 'aggiorna', azione: () => { invia(stato.sessione.id, domanda.corpo.testo, { provider: p.id }); avvisa(`Riprovo con ${nomeProvider(p.id)}`); },
    }))] : []),
  ].filter(Boolean));
}

export function rigaDa(id) { return righe.get(id); }
