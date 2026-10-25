// Il computer dell'assistente sulla VPS (Fase 2.H, disegno approvato il 2026-10-25 06:50).
// Scheda Computer del pannello: intestazione con stato, risoluzione e «da N min»; sotto-schede
// Schermo (noVNC sul WebSocket di websockify) e Terminale (la pagina di ttyd in un iframe);
// «guardo» (viewOnly, predefinito) e «prendo i comandi» (tastiera e mouse, bordo colorato);
// «⛶» porta la stessa vista sopra tutta la finestra (Esc esce), «×» chiude la scheda.
// Chiudere la vista chiude solo la connessione RFB: il computer resta acceso finché non lo si
// spegne dal foglio dell'assistente (scheda Computer: acceso/spento, «Ricrea»).
// 2.H-bis (prova dal telefono, 2026-10-25): tastiera del telefono (campo nascosto → rfb.sendKey) con la
// barra dei tasti speciali, pizzico e due dita per ingrandire e spostare (scala CSS 1×–4×, la
// posizione del mouse corretta per la scala), doppio tocco «adatta ↔ 1:1» in «guardo», tasti
// + / − / adatta, menu con Ctrl+Alt+Canc e Incolla, tutto schermo che blocca lo scorrimento.

import RFB from './vendor/novnc/core/rfb.js';
import KeyTable from './vendor/novnc/core/input/keysym.js';
import keysyms from './vendor/novnc/core/input/keysymdef.js';
import { api, ErroreRete, testoErrore } from './api.js';
import { icona } from './icone.js';
import { assistenteDi, bus, stato } from './stato.js';
import { apriMenu, chiediTesto } from './menu.js';
import { interruttore, nota } from './ui.js';
import { avvisa, el, livelloIndietro } from './util.js';

export const STATI_COMPUTER = { acceso: 'acceso', avvio: 'si accende', spento: 'spento', errore: 'in errore' };
const COLORI = { acceso: 'verde', avvio: 'giallo', spento: 'grigio', errore: 'rosso' };
const OGNI_MS = 30000; // l'intestazione si rilegge ogni 30 s mentre la vista è aperta
const AVVIO_MS = 2000; // durante l'avvio ogni 2 s

/** Il pallino dello stato del computer (pannello, Squadra, foglio). */
export function puntoComputer(st, testo = true) {
  const s = st || 'spento';
  return el(`span.punto-${COLORI[s] || 'grigio'}.punto-computer`, { role: 'img', 'aria-label': `Computer ${STATI_COMPUTER[s] || s}`, title: testo ? `Computer ${STATI_COMPUTER[s] || s}` : undefined });
}

const daQuanto = (ms) => {
  if (!ms) return '';
  const min = Math.max(0, Math.round((Date.now() - ms) / 60000));
  return min < 1 ? 'da meno di un minuto' : min < 120 ? `da ${min} min` : `da ${Math.round(min / 60)} ore`;
};

const urlWs = (percorso) => `${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}${percorso}`;

function aggiornaStatoInMemoria(aid, c) {
  const a = stato.assistenti.get(aid); // l'oggetto in memoria: assistenteDi() ne dà una copia
  if (a && c?.stato && a.computer?.stato !== c.stato) { a.computer = { ...(a.computer || {}), stato: c.stato }; bus.emetti('computer', { aid, stato: c.stato }); }
}

// Una sola vista viva per volta: sopravvive ai ridisegni del pannello (eventi «sessione»,
// «assistente:aggiornato») e al passaggio a tutto schermo, perché il nodo si sposta, non si rifà.
let vista = null;

/** Vero se la vista è a tutto schermo (il pannello chiuso non la chiude). */
export const vistaAPieno = () => !!vista?.pieno;

/** Chiude la vista (connessione RFB, iframe, tutto schermo). Il computer non si tocca. */
export function chiudiVista() {
  if (!vista) return;
  const v = vista;
  vista = null;
  clearTimeout(v.timer);
  chiudiTastiera(v);
  esciTuttoSchermo(v);
  try { v.rfb?.disconnect(); } catch { /* già chiusa */ }
  v.rfb = null;
  v.nodo.remove();
}

/** La scheda Computer del pannello per l'assistente `aid`. `chiudi()` torna alla scheda Dettagli. */
export function sezioneSchermo(aid, chiudi) {
  if (vista && vista.aid !== aid) chiudiVista();
  if (!vista) vista = creaVista(aid, chiudi);
  vista.chiudi = chiudi;
  if (!vista.pieno) {
    vista.casa = el('div.computer-casa');
    vista.casa.append(vista.nodo);
    riscala(vista);
  } else {
    // a tutto schermo la vista resta sopra; nel pannello un segnaposto
    vista.casa = el('div.computer-casa', {}, el('p.nota', {}, 'Il computer è a tutto schermo.'));
  }
  if (!vista.dati) leggi(vista); else disegnaTesta(vista);
  return vista.casa;
}

function creaVista(aid, chiudi) {
  const v = { aid, chiudi, dati: null, rfb: null, sotto: 'schermo', comando: false, pieno: null, timer: 0, iframe: null, collegato: false,
    zoom: { z: 1, tx: 0, ty: 0 }, mod: { ctrl: false, alt: false, super: false }, tastieraAperta: false };
  v.testa = el('div.computer-testa');
  v.tasti = el('div.computer-tasti');
  v.schermo = el('div.computer-schermo', { 'aria-label': 'Schermo del computer' });
  v.terminale = el('div.computer-terminale', { hidden: true });
  v.messaggio = el('div.computer-messaggio', { role: 'status' });
  v.segnoComando = el('span.computer-segno', { 'aria-hidden': 'true' }, 'comando');
  v.segnoZoom = el('button.computer-segno-zoom', { type: 'button', hidden: true, title: 'Torna ad adatta', onclick: () => adatta(v) });
  v.area = el('div.computer-area', { dati: { zoom: 'no' } }, v.schermo, v.terminale, v.messaggio, v.segnoComando, v.segnoZoom);
  v.campo = el('textarea.computer-campo', {
    'aria-label': 'Tastiera del computer', autocapitalize: 'off', autocorrect: 'off', autocomplete: 'off', spellcheck: 'false',
    inputmode: 'text', enterkeyhint: 'enter', rows: 1, tabindex: '-1',
  });
  v.barraTasti = el('div.computer-tastiera', { hidden: true, role: 'toolbar', 'aria-label': 'Tasti speciali' });
  preparaCampo(v);
  preparaGesti(v);
  v.verticale = el('p.nota.computer-verticale', {}, 'Gira il telefono in orizzontale: lo schermo si vede meglio.');
  v.nodo = el('div.computer-vista', { dati: { comando: 'no' } }, v.testa, v.tasti, v.area, v.barraTasti, v.campo, v.verticale);
  return v;
}

async function leggi(v, azione) {
  clearTimeout(v.timer);
  let c;
  try {
    c = azione ? await api.azioneComputer(v.aid, azione) : await api.computer(v.aid);
  } catch (e) {
    if (vista !== v) return;
    if (azione) { avvisa(testoErrore(e)); v.dati = v.dati || { stato: 'spento' }; disegna(v); return; }
    // 2.L difetto 1: la rete che manca (schermo spento, telefono che dorme) non è un computer «in errore»:
    // resta l'ultimo stato letto, la testa dice che riprovo e si riprova solo a pagina visibile.
    if (e instanceof ErroreRete) {
      v.rete = true;
      if (!v.dati) v.dati = { stato: 'spento' };
      disegnaTesta(v);
      if (!document.hidden) v.timer = setTimeout(() => { if (vista === v && v.nodo.isConnected) leggi(v); }, AVVIO_MS * 5);
      return;
    }
    v.dati = { ...(v.dati || {}), stato: 'errore', errore: testoErrore(e) };
    disegna(v);
    return;
  }
  if (vista !== v) return;
  v.rete = false;
  v.dati = c || { stato: 'spento' };
  aggiornaStatoInMemoria(v.aid, v.dati);
  disegna(v);
  if (!document.hidden) v.timer = setTimeout(() => { if (vista === v && v.nodo.isConnected) leggi(v); }, v.dati.stato === 'avvio' ? AVVIO_MS : OGNI_MS);
}

// 2.L difetti 1 e 11: a pagina nascosta la testa non si rilegge; al ritorno si rilegge subito e l'errore di rete sparisce.
document.addEventListener('visibilitychange', () => {
  if (!vista) return;
  clearTimeout(vista.timer);
  if (!document.hidden && vista.nodo.isConnected) { vista.rete = false; leggi(vista); }
});

function disegnaTesta(v) {
  const a = assistenteDi(v.aid);
  const c = v.dati || {};
  const dettagli = [STATI_COMPUTER[c.stato] || c.stato || '…',
    c.larghezza && c.altezza ? `${c.larghezza}×${c.altezza}` : null,
    c.stato === 'acceso' ? daQuanto(c.acceso_ms) : null, v.rete ? 'server non raggiungibile, riprovo' : null].filter(Boolean).join(' · ');
  v.testa.replaceChildren(
    el('span.computer-nome', {}, el('strong', {}, a?.nome || 'Assistente'), el('span', {}, ' · computer sulla VPS')),
    el('span.computer-stato', {}, puntoComputer(c.stato, false), el('span', {}, dettagli)));
  const sotto = (id, t) => el('button', { type: 'button', role: 'tab', 'aria-selected': v.sotto === id ? 'true' : 'false', onclick: () => { v.sotto = id; mostraSotto(v); disegnaTesta(v); } }, t);
  const acceso = c.stato === 'acceso';
  const tasto = (ic, testo, etichetta, onclick, attivo, extra = '') => el(`button.tasto.piccolo${extra}`, {
    type: 'button', 'aria-label': etichetta, title: etichetta, 'aria-pressed': attivo === undefined ? undefined : String(!!attivo), onclick, disabled: !acceso && attivo !== undefined,
  }, ic ? icona(ic, 14) : null, testo);
  v.tasti.replaceChildren(
    el('div.schede-pillola.computer-sotto', { role: 'tablist', 'aria-label': 'Schermo o terminale' }, sotto('schermo', 'Schermo'), sotto('terminale', 'Terminale')),
    el('span.spazio'),
    tasto('occhio', 'guardo', 'Guardo soltanto', () => comanda(v, false), !v.comando),
    tasto('presa', 'prendo i comandi', 'Prendo i comandi', () => comanda(v, true), v.comando, '.comando'),
    v.sotto === 'terminale'
      ? el('button.tasto.piccolo', { type: 'button', 'aria-label': 'Tastiera del terminale', title: 'Apre la tastiera sul terminale', onclick: () => tastieraTerminale(v), disabled: !acceso }, icona('tastiera', 14), 'Tastiera')
      : el('button.tasto.piccolo', { type: 'button', 'aria-label': 'Tastiera', title: 'Apre la tastiera del telefono', 'aria-pressed': String(v.tastieraAperta), onclick: () => apriTastiera(v), disabled: !acceso }, icona('tastiera', 14), 'Tastiera'),
    ...tastiZoom(v, !acceso || v.sotto === 'terminale'),
    el('button.tasto-icona.piccolo', { type: 'button', 'aria-label': 'Altri comandi', title: 'Ctrl+Alt+Canc, Incolla, 1:1', onclick: (ev) => menuAltro(v, ev.currentTarget), disabled: !acceso }, icona('altro', 14)),
    el('button.tasto-icona.piccolo', { type: 'button', 'aria-label': v.pieno ? 'Esci dal tutto schermo' : 'A tutto schermo', title: v.pieno ? 'Esci dal tutto schermo' : 'A tutto schermo', onclick: () => (v.pieno ? esciTuttoSchermo(v) : tuttoSchermo(v)) }, '⛶'),
    v.pieno ? null : el('button.tasto-icona.piccolo', { type: 'button', 'aria-label': 'Chiudi la scheda Computer', title: 'Chiudi (il computer resta acceso)', onclick: () => { const f = v.chiudi; chiudiVista(); f?.(); } }, icona('chiudi', 14)));
  if (v.pieno) disegnaBarraPiena(v);
}

function disegna(v) {
  disegnaTesta(v);
  const c = v.dati || {};
  v.messaggio.hidden = c.stato === 'acceso' && v.collegato;
  if (c.stato === 'acceso') {
    if (!v.rfb) { v.messaggio.replaceChildren(el('p.nota', {}, 'Mi collego allo schermo…')); collega(v); }
  } else {
    if (v.rfb) { try { v.rfb.disconnect(); } catch { /* */ } v.rfb = null; v.collegato = false; }
    v.schermo.replaceChildren();
    v.messaggio.hidden = false;
    if (c.stato === 'avvio') v.messaggio.replaceChildren(el('p.computer-attesa', {}, 'Il computer si accende', el('span.puntini', { 'aria-hidden': 'true' }, el('i'), el('i'), el('i'))));
    else if (c.stato === 'errore') v.messaggio.replaceChildren(el('p.nota', {}, c.errore || 'Il computer non è partito.'), el('button.tasto.piccolo', { type: 'button', onclick: () => ricrea(v) }, icona('aggiorna', 14), 'Ricrea'));
    else v.messaggio.replaceChildren(el('p.nota', {}, 'Il computer è spento.'), el('button.tasto.primario.piccolo', { type: 'button', onclick: () => { v.dati = { ...c, stato: 'avvio' }; disegna(v); leggi(v, 'accendi'); } }, icona('computer', 14), 'Accendi il computer'));
  }
  mostraSotto(v);
}

function mostraSotto(v) {
  const terminale = v.sotto === 'terminale';
  v.schermo.hidden = terminale;
  v.terminale.hidden = !terminale;
  if (terminale && v.dati?.stato === 'acceso' && !v.iframe && v.dati.terminale) {
    v.iframe = el('iframe.computer-iframe', { src: v.dati.terminale, title: 'Terminale dell\'assistente (tmux)', allow: 'clipboard-read; clipboard-write' });
    v.terminale.replaceChildren(v.iframe);
  }
  if (terminale && v.dati?.stato !== 'acceso') { v.iframe = null; v.terminale.replaceChildren(); }
  if (!terminale) riscala(v);
}

function collega(v) {
  const c = v.dati;
  if (!c?.schermo) { v.messaggio.replaceChildren(el('p.nota', {}, 'Il server non ha dato l\'indirizzo dello schermo.')); return; }
  let rfb;
  try {
    rfb = new RFB(v.schermo, urlWs(c.schermo), { shared: true });
  } catch (e) { v.messaggio.replaceChildren(el('p.nota', {}, `Lo schermo non si apre: ${e.message || e}`)); return; }
  v.rfb = rfb;
  rfb.viewOnly = !v.comando;
  rfb.scaleViewport = true;
  rfb.resizeSession = false;
  rfb.focusOnClick = true;
  rfb.background = 'transparent';
  rfb.dragViewport = false;
  correggiScala(v, rfb);
  rfb.addEventListener('connect', () => { if (v.rfb !== rfb) return; v.collegato = true; v.messaggio.hidden = true; riscala(v); });
  rfb.addEventListener('disconnect', (ev) => {
    if (v.rfb !== rfb) return;
    v.rfb = null;
    v.collegato = false;
    if (vista !== v) return;
    v.messaggio.hidden = false;
    v.messaggio.replaceChildren(el('p.nota', {}, ev.detail?.clean ? 'Connessione allo schermo chiusa.' : 'Lo schermo non risponde.'),
      el('button.tasto.piccolo', { type: 'button', onclick: () => { v.messaggio.replaceChildren(el('p.nota', {}, 'Mi collego allo schermo…')); leggi(v); } }, icona('aggiorna', 14), 'Ricollega'));
  });
  rfb.addEventListener('securityfailure', (ev) => { v.messaggio.hidden = false; v.messaggio.replaceChildren(el('p.nota', {}, `Accesso allo schermo rifiutato${ev.detail?.reason ? `: ${ev.detail.reason}` : ''}.`)); });
  rfb.addEventListener('credentialsrequired', () => { try { rfb.disconnect(); } catch { /* */ } });
}

function comanda(v, si) {
  v.comando = si;
  v.nodo.dataset.comando = si ? 'si' : 'no';
  if (v.rfb) { v.rfb.viewOnly = !si; if (si && !v.tastieraAperta) v.rfb.focus(); }
  if (!si) chiudiTastiera(v);
  applicaZoom(v);
  disegnaTesta(v);
  if (v.pieno) disegnaBarraPiena(v);
}

/** noVNC ricalcola la scala sul ridimensionamento: dopo uno spostamento del nodo la forziamo. */
function riscala(v) {
  if (!v.rfb) return;
  requestAnimationFrame(() => { if (!v.rfb) return; v.rfb.scaleViewport = false; v.rfb.scaleViewport = true; });
}

async function ricrea(v) {
  v.dati = { ...(v.dati || {}), stato: 'avvio' };
  disegna(v);
  await leggi(v, 'ricrea');
}

// ---------- tutto schermo ----------
function tuttoSchermo(v) {
  if (v.pieno) return;
  const barra = el('div.computer-pieno-barra');
  const strato = el('div.computer-pieno', { role: 'dialog', 'aria-modal': 'true', 'aria-label': 'Computer a tutto schermo' }, barra);
  const esc = (ev) => { if (ev.key === 'Escape') { ev.preventDefault(); ev.stopPropagation(); esciTuttoSchermo(v); } };
  v.pieno = { strato, barra, esc, casa: v.casa };
  document.addEventListener('keydown', esc, true);
  document.documentElement.classList.add('computer-a-pieno');
  // Indietro di Android: il primo lo consuma il sistema per chiudere la tastiera (nessun popstate),
  // quindi il popstate che arriva qui esce sempre dal tutto schermo. Rimettere la voce dentro popstate
  // non serve: senza un tocco Chrome la marca «da saltare» e l'Indietro dopo chiude l'app.
  v.pieno.togliIndietro = livelloIndietro(() => {
    chiudiTastiera(v);
    if (v.pieno) { v.pieno.togliIndietro = null; esciTuttoSchermo(v); }
  });
  v.pieno.vv = () => seguiViewport(v);
  window.visualViewport?.addEventListener('resize', v.pieno.vv);
  window.visualViewport?.addEventListener('scroll', v.pieno.vv);
  v.casa.replaceChildren(el('p.nota', {}, 'Il computer è a tutto schermo.'));
  strato.append(v.nodo);
  document.body.append(strato);
  disegnaBarraPiena(v);
  disegnaTesta(v);
  riscala(v);
  if (v.comando) v.rfb?.focus();
}

function disegnaBarraPiena(v) {
  if (!v.pieno) return;
  const a = assistenteDi(v.aid);
  const c = v.dati || {};
  const modo = (id, t, si) => el('button', { type: 'button', role: 'tab', 'aria-selected': String(v.comando === si), onclick: () => comanda(v, si) }, t);
  v.pieno.barra.replaceChildren(
    // riga 1: nome e pallino a sinistra (si accorciano), «guardo | comando» al centro, «esci» a destra sempre visibile
    el('div.pieno-chi', {}, el('strong', {}, a?.nome || 'Assistente'), el('span.nota', {}, '· computer'), puntoComputer(c.stato)),
    el('div.schede-pillola', { role: 'tablist', 'aria-label': 'Guardo o comando' }, modo('guardo', 'guardo', false), modo('comando', 'comando', true)),
    el('button.tasto.piccolo.pieno-esci', { type: 'button', onclick: () => esciTuttoSchermo(v), 'aria-label': 'Esci dal tutto schermo' }, 'esci ⛶'),
    // riga 2 sul telefono (scorrevole), in coda alla riga 1 sullo schermo largo
    el('div.pieno-attrezzi', {},
      el('button.tasto-icona.piccolo', { type: 'button', 'aria-label': 'Tastiera', title: 'Tastiera', 'aria-pressed': String(v.tastieraAperta), onclick: () => (v.sotto === 'terminale' ? tastieraTerminale(v) : apriTastiera(v)) }, icona('tastiera', 16)),
      ...tastiZoom(v, v.sotto === 'terminale'),
      el('button.tasto-icona.piccolo', { type: 'button', 'aria-label': 'Altri comandi', onclick: (ev) => menuAltro(v, ev.currentTarget) }, icona('altro', 16))));
}

function esciTuttoSchermo(v) {
  if (!v.pieno) return;
  v.pieno.strato.style.height = '';
  v.pieno.strato.style.top = '';
  const { strato, esc } = v.pieno;
  document.removeEventListener('keydown', esc, true);
  document.documentElement.classList.remove('computer-a-pieno');
  v.pieno.togliIndietro?.();
  window.visualViewport?.removeEventListener('resize', v.pieno.vv);
  window.visualViewport?.removeEventListener('scroll', v.pieno.vv);
  const casa = v.casa;
  v.pieno = null;
  strato.remove();
  if (casa?.isConnected) { casa.replaceChildren(v.nodo); disegnaTesta(v); riscala(v); } else if (vista === v) { chiudiVista(); }
}

// ---------- 2.H-bis: zoom e spostamento (pizzico, due dita, doppio tocco, + / − / adatta) ----------
const ZOOM_MAX = 4;

/** noVNC non sa della nostra scala CSS: la posizione del mouse va divisa per lo zoom. */
function correggiScala(v, rfb) {
  const d = rfb._display; // noVNC 1.3 (copia in vendor/): absX/absY convertono i px del canvas in px del computer
  if (!d || d.__xbotZoom) return;
  const ax = d.absX.bind(d);
  const ay = d.absY.bind(d);
  d.absX = (x) => ax(x / v.zoom.z);
  d.absY = (y) => ay(y / v.zoom.z);
  d.__xbotZoom = true;
}

function limita(v) {
  const W = v.area.clientWidth;
  const H = v.area.clientHeight;
  const z = v.zoom;
  z.z = Math.min(ZOOM_MAX, Math.max(1, z.z));
  z.tx = Math.min(0, Math.max(W - W * z.z, z.tx));
  z.ty = Math.min(0, Math.max(H - H * z.z, z.ty));
}

function applicaZoom(v) {
  limita(v);
  const { z, tx, ty } = v.zoom;
  const ingrandito = z > 1.01;
  v.schermo.style.transform = ingrandito ? `translate(${tx}px, ${ty}px) scale(${z})` : '';
  if (!ingrandito) v.zoom = { z: 1, tx: 0, ty: 0 };
  v.area.dataset.zoom = ingrandito ? 'si' : 'no';
  v.segnoZoom.hidden = !ingrandito;
  v.segnoZoom.textContent = `${z.toLocaleString('it-IT', { maximumFractionDigits: 1 })}×`;
  // «guardo» senza zoom: un dito scorre la pagina; altrimenti i gesti sono nostri o di noVNC
  const tocco = !v.comando && !ingrandito && !v.pieno ? 'pan-y' : 'none';
  v.area.style.touchAction = tocco;
  const canvas = v.schermo.querySelector('canvas');
  if (canvas) canvas.style.touchAction = tocco;
}

/** Porta lo zoom a `z2` tenendo fermo il punto (cx, cy) dell'area. */
function zoomA(v, z2, cx, cy) {
  const z = v.zoom;
  const nuovo = Math.min(ZOOM_MAX, Math.max(1, z2));
  if (cx === undefined) { cx = v.area.clientWidth / 2; cy = v.area.clientHeight / 2; }
  z.tx = cx - (cx - z.tx) * (nuovo / z.z);
  z.ty = cy - (cy - z.ty) * (nuovo / z.z);
  z.z = nuovo;
  applicaZoom(v);
}

const adatta = (v) => { v.zoom = { z: 1, tx: 0, ty: 0 }; applicaZoom(v); };

/** 1:1 = un pixel del computer per un pixel dello schermo (almeno 2× se lo schermo è già grande). */
function unoAUno(v, cx, cy) {
  const scala = v.rfb?._display?.scale || 1;
  zoomA(v, Math.max(2, Math.min(ZOOM_MAX, 1 / scala)), cx, cy);
}

function tastiZoom(v, spento) {
  const t = (ic, etichetta, f) => el('button.tasto-icona.piccolo', { type: 'button', 'aria-label': etichetta, title: etichetta, onclick: f, disabled: spento }, icona(ic, 14));
  return [t('meno', 'Rimpicciolisci', () => zoomA(v, v.zoom.z / 1.5)), t('piu', 'Ingrandisci', () => zoomA(v, v.zoom.z * 1.5)), t('adatta', 'Adatta allo spazio', () => adatta(v))];
}

const punto = (v, t) => { const r = v.area.getBoundingClientRect(); return { x: t.clientX - r.left, y: t.clientY - r.top }; };

/**
 * I gesti sul contenitore (fase di cattura, prima di noVNC):
 * - due dita: pizzico = zoom centrato, trascinare = spostare (in guardo e in comando);
 * - un dito in «guardo»: con lo zoom sposta la vista, senza zoom scorre la pagina; doppio tocco adatta ↔ 1:1;
 * - un dito in «comando»: va a noVNC (tocco = clic, tocco lungo = clic destro, trascina = trascina).
 * Il pizzico e le due dita di noVNC (rotella e Ctrl+rotella sul computer) si fermano qui.
 */
function preparaGesti(v) {
  let g = null; // {tipo: 'due'|'uno', ...}
  let ultimoTocco = { t: 0, x: 0, y: 0 };
  const area = v.area;
  area.addEventListener('touchstart', (ev) => {
    if (v.sotto !== 'schermo') return;
    const tt = ev.touches;
    if (tt.length >= 2) {
      const a = punto(v, tt[0]);
      const b = punto(v, tt[1]);
      g = { tipo: 'due', d0: Math.hypot(a.x - b.x, a.y - b.y) || 1, c0: { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 }, ...v.zoom, z0: v.zoom.z };
      ev.stopPropagation();
      ev.preventDefault();
    } else if (!v.comando) {
      const a = punto(v, tt[0]);
      g = { tipo: 'uno', x0: a.x, y0: a.y, tx: v.zoom.tx, ty: v.zoom.ty, mosso: false };
      ev.stopPropagation(); // in guardo noVNC non riceve il tocco
    }
  }, { capture: true, passive: false });
  area.addEventListener('touchmove', (ev) => {
    if (!g) return;
    const tt = ev.touches;
    if (g.tipo === 'due' && tt.length >= 2) {
      const a = punto(v, tt[0]);
      const b = punto(v, tt[1]);
      const z = Math.min(ZOOM_MAX, Math.max(1, g.z0 * (Math.hypot(a.x - b.x, a.y - b.y) / g.d0)));
      const c = { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
      // il punto del computer che stava sotto il centro iniziale segue il centro delle dita
      v.zoom.z = z;
      v.zoom.tx = c.x - ((g.c0.x - g.tx) / g.z0) * z;
      v.zoom.ty = c.y - ((g.c0.y - g.ty) / g.z0) * z;
      applicaZoom(v);
      ev.stopPropagation();
      ev.preventDefault();
    } else if (g.tipo === 'uno' && tt.length === 1) {
      const a = punto(v, tt[0]);
      if (Math.hypot(a.x - g.x0, a.y - g.y0) > 8) g.mosso = true;
      ev.stopPropagation();
      if (v.zoom.z > 1.01) {
        v.zoom.tx = g.tx + (a.x - g.x0);
        v.zoom.ty = g.ty + (a.y - g.y0);
        applicaZoom(v);
        ev.preventDefault();
      }
    } else if (g.tipo === 'due') {
      ev.stopPropagation();
    }
  }, { capture: true, passive: false });
  const fine = (ev) => {
    if (!g) return;
    if (g.tipo === 'uno' && !g.mosso && ev.type === 'touchend' && ev.changedTouches[0]) {
      const a = punto(v, ev.changedTouches[0]);
      const ora = Date.now();
      if (ora - ultimoTocco.t < 320 && Math.hypot(a.x - ultimoTocco.x, a.y - ultimoTocco.y) < 30) {
        if (v.zoom.z > 1.01) adatta(v); else unoAUno(v, a.x, a.y);
        ultimoTocco = { t: 0, x: 0, y: 0 };
        ev.preventDefault();
      } else ultimoTocco = { t: ora, x: a.x, y: a.y };
    }
    if (g.tipo === 'due' || !v.comando) ev.stopPropagation();
    if (ev.touches.length === 0) g = null;
    else if (g.tipo === 'due' && ev.touches.length === 1) g = { tipo: 'finito' }; // un dito resta giù: niente clic
  };
  area.addEventListener('touchend', fine, { capture: true, passive: false });
  area.addEventListener('touchcancel', fine, { capture: true, passive: false });
  // i gesti a due dita di noVNC (rotella, Ctrl+rotella) non arrivano al computer: le due dita sono nostre
  for (const tipo of ['gesturestart', 'gesturemove', 'gestureend']) {
    v.schermo.addEventListener(tipo, (ev) => { if (['pinch', 'twodrag'].includes(ev.detail?.type)) ev.stopPropagation(); }, true);
  }
  // Ctrl+rotella sul Mac (e il pizzico del trackpad) = zoom locale
  area.addEventListener('wheel', (ev) => {
    if (!ev.ctrlKey || v.sotto !== 'schermo') return;
    ev.preventDefault();
    ev.stopPropagation();
    const a = punto(v, ev);
    zoomA(v, v.zoom.z * Math.exp(-ev.deltaY / 200), a.x, a.y);
  }, { capture: true, passive: false });
  new ResizeObserver(() => applicaZoom(v)).observe(area);
}

// ---------- 2.H-bis: la tastiera del telefono e i tasti speciali ----------
const SPECIALI = {
  Enter: KeyTable.XK_Return, Backspace: KeyTable.XK_BackSpace, Tab: KeyTable.XK_Tab, Escape: KeyTable.XK_Escape,
  Delete: KeyTable.XK_Delete, Home: KeyTable.XK_Home, End: KeyTable.XK_End, PageUp: KeyTable.XK_Prior, PageDown: KeyTable.XK_Next,
  ArrowLeft: KeyTable.XK_Left, ArrowUp: KeyTable.XK_Up, ArrowRight: KeyTable.XK_Right, ArrowDown: KeyTable.XK_Down,
};
const MODIFICATORI = { ctrl: [KeyTable.XK_Control_L, 'ControlLeft'], alt: [KeyTable.XK_Alt_L, 'AltLeft'], super: [KeyTable.XK_Super_L, 'MetaLeft'] };
const SENTINELLA = '        '; // il campo non è mai vuoto: Backspace cancella e lo vediamo

function keysymDi(car) {
  if (car === '\n' || car === '\r') return KeyTable.XK_Return;
  if (car === '\t') return KeyTable.XK_Tab;
  return keysyms.lookup(car.codePointAt(0));
}

/** Manda un tasto (giù e su) con i modificatori attivi; i modificatori «una volta» poi si spengono. */
function premi(v, keysym, extra = {}) {
  if (!v.rfb || !v.comando || !keysym) return;
  const mod = { ...v.mod, ...extra };
  const attivi = Object.keys(MODIFICATORI).filter((k) => mod[k]);
  for (const k of attivi) v.rfb.sendKey(MODIFICATORI[k][0], MODIFICATORI[k][1], true);
  v.rfb.sendKey(keysym, null);
  for (const k of attivi.reverse()) v.rfb.sendKey(MODIFICATORI[k][0], MODIFICATORI[k][1], false);
  if (Object.values(v.mod).some(Boolean)) { v.mod = { ctrl: false, alt: false, super: false }; disegnaTastiera(v); }
}

function scrivi(v, testo) {
  for (const car of testo) premi(v, keysymDi(car));
}

function azzeraCampo(v) {
  v.campo.value = SENTINELLA;
  v.campo.setSelectionRange(SENTINELLA.length, SENTINELLA.length);
  v.ultimo = SENTINELLA;
}

function preparaCampo(v) {
  const c = v.campo;
  azzeraCampo(v);
  let componendo = false;
  c.addEventListener('compositionstart', () => { componendo = true; });
  c.addEventListener('compositionend', () => { componendo = false; setTimeout(() => azzeraCampo(v), 0); });
  c.addEventListener('keydown', (ev) => {
    if (ev.isComposing || ev.keyCode === 229) return; // la tastiera del telefono: decide il campo
    const ks = SPECIALI[ev.key];
    const conMod = ev.ctrlKey || ev.altKey || ev.metaKey;
    if (ks || (conMod && ev.key.length === 1)) {
      ev.preventDefault();
      ev.stopPropagation();
      premi(v, ks || keysymDi(ev.key), { ctrl: ev.ctrlKey || v.mod.ctrl, alt: ev.altKey || v.mod.alt, super: ev.metaKey || v.mod.super });
    }
  });
  c.addEventListener('input', () => {
    const prima = v.ultimo ?? SENTINELLA;
    const ora = c.value;
    let i = 0;
    while (i < prima.length && i < ora.length && prima[i] === ora[i]) i++;
    for (let k = 0; k < prima.length - i; k++) premi(v, KeyTable.XK_BackSpace);
    scrivi(v, ora.slice(i));
    v.ultimo = ora;
    if (!componendo) azzeraCampo(v); // durante la composizione (Gboard) il campo resta com'è fino alla fine
  });
  c.addEventListener('blur', () => {
    // il fuoco va a un tasto della barra (pointerdown lo tiene) o fuori: fuori la tastiera si chiude
    setTimeout(() => { if (v.tastieraAperta && document.activeElement !== c) chiudiTastiera(v); }, 150);
  });
}

function disegnaTastiera(v) {
  const t = (testo, etichetta, f, extra = {}) => el('button.tasto.piccolo', {
    type: 'button', 'aria-label': etichetta, title: etichetta, ...extra,
    onpointerdown: (ev) => ev.preventDefault(), // il campo resta a fuoco: la tastiera non si chiude
    onclick: () => { f(); v.campo.focus({ preventScroll: true }); },
  }, testo);
  const mod = (k, testo, etichetta) => t(testo, etichetta, () => { v.mod[k] = !v.mod[k]; disegnaTastiera(v); }, { 'aria-pressed': String(v.mod[k]), class: 'tasto piccolo modificatore' });
  v.barraTasti.replaceChildren(
    t('Esc', 'Esc', () => premi(v, KeyTable.XK_Escape)),
    t('Tab', 'Tab', () => premi(v, KeyTable.XK_Tab)),
    mod('ctrl', 'Ctrl', 'Ctrl (una volta)'), mod('alt', 'Alt', 'Alt (una volta)'), mod('super', '⌘', 'Super / ⌘ (una volta)'),
    t('←', 'Freccia a sinistra', () => premi(v, KeyTable.XK_Left)),
    t('↑', 'Freccia in su', () => premi(v, KeyTable.XK_Up)),
    t('↓', 'Freccia in giù', () => premi(v, KeyTable.XK_Down)),
    t('→', 'Freccia a destra', () => premi(v, KeyTable.XK_Right)),
    t('Invio', 'Invio', () => premi(v, KeyTable.XK_Return)),
    t('Ctrl+Alt+Canc', 'Ctrl+Alt+Canc', () => v.rfb?.sendCtrlAltDel()),
    t(icona('appunti', 14), 'Incolla dagli appunti', () => incolla(v)),
    el('button.tasto-icona.piccolo', { type: 'button', 'aria-label': 'Chiudi la tastiera', title: 'Chiudi la tastiera', onclick: () => chiudiTastiera(v) }, icona('giu', 14)));
}

function apriTastiera(v) {
  if (v.dati?.stato !== 'acceso' || !v.rfb) return;
  if (!v.comando) comanda(v, true);
  // sul telefono la tastiera copre metà schermo: la vista va a tutto schermo e segue lo spazio rimasto
  if (!v.pieno && matchMedia('(max-width: 767px)').matches) tuttoSchermo(v);
  v.tastieraAperta = true;
  azzeraCampo(v);
  v.campo.focus({ preventScroll: true }); // dentro il tocco: il telefono apre la tastiera
  v.barraTasti.hidden = false;
  disegnaTastiera(v);
  disegnaTesta(v);
}

function chiudiTastiera(v) {
  if (!v.tastieraAperta) return;
  v.tastieraAperta = false;
  v.barraTasti.hidden = true;
  v.mod = { ctrl: false, alt: false, super: false };
  if (document.activeElement === v.campo) v.campo.blur();
  if (vista === v) disegnaTesta(v);
}

/** A tutto schermo con la tastiera aperta la vista prende solo lo spazio visibile sopra la tastiera. */
function seguiViewport(v) {
  const vv = window.visualViewport;
  if (!v.pieno || !vv) return;
  v.pieno.strato.style.height = `${vv.height}px`;
  v.pieno.strato.style.top = `${vv.offsetTop}px`;
}

async function incolla(v) {
  if (!v.rfb || !v.comando) { avvisa('Prima «prendo i comandi»'); return; }
  let testo = '';
  try { testo = await navigator.clipboard.readText(); } catch { /* permesso negato o browser senza appunti */ }
  if (!testo) testo = await chiediTesto({ titolo: 'Incolla sul computer', etichetta: 'Testo da scrivere', conferma: 'Scrivi', massimo: 4000 }) || '';
  if (!testo) return;
  if (testo.length > 4000) { avvisa('Testo troppo lungo: al massimo 4000 caratteri'); return; }
  try { v.rfb.clipboardPasteFrom(testo); } catch { /* appunti del computer: facoltativi */ }
  scrivi(v, testo);
  avvisa(`Scritti ${testo.length} caratteri`);
}

function menuAltro(v, ancora) {
  const acceso = v.dati?.stato === 'acceso' && !!v.rfb;
  apriMenu(ancora, [
    { titolo: 'Computer' },
    { testo: 'Ctrl+Alt+Canc', icona: 'tastiera', disattivo: !acceso || !v.comando, azione: () => v.rfb?.sendCtrlAltDel() },
    { testo: 'Incolla dagli appunti', icona: 'appunti', disattivo: !acceso, azione: () => { if (!v.comando) comanda(v, true); incolla(v); } },
    '-',
    { testo: 'Zoom 1:1', icona: 'piu', disattivo: !acceso, azione: () => unoAUno(v) },
    { testo: 'Adatta allo spazio', icona: 'adatta', disattivo: !acceso, azione: () => adatta(v) },
  ]);
}

/** Terminale (ttyd/xterm nell'iframe): il tocco sul terminale apre già la tastiera; il tasto la forza. */
function tastieraTerminale(v) {
  const f = v.iframe;
  if (!f) return;
  try {
    f.focus();
    const campo = f.contentDocument?.querySelector('.xterm-helper-textarea, textarea');
    if (campo) campo.focus(); else f.contentWindow?.focus();
  } catch { f.focus(); /* altra origine: basta il fuoco all'iframe */ }
}

// ---------- foglio dell'assistente: scheda «Computer» ----------
export function schedaComputer(aid) {
  const rigaStato = el('span.computer-stato');
  const errore = el('p.nota', { hidden: true });
  let dati = null;
  let giro = 0;
  const sw = interruttore('Computer acceso', false, async (si, nodo) => {
    nodo.disabled = true;
    try { scrivi(await api.azioneComputer(aid, si ? 'accendi' : 'spegni')); avvisa(si ? 'Computer in accensione' : 'Computer spento'); } catch (e) { nodo.checked = !si; avvisa(testoErrore(e)); } finally { nodo.disabled = false; }
  });
  const ricreaT = el('button.tasto.piccolo', { type: 'button', onclick: async () => {
    ricreaT.disabled = true;
    try { scrivi(await api.azioneComputer(aid, 'ricrea')); avvisa('Computer ricreato'); } catch (e) { avvisa(testoErrore(e)); } finally { ricreaT.disabled = false; }
  } }, icona('aggiorna', 14), 'Ricrea');
  function scrivi(c) {
    dati = c || { stato: 'spento' };
    aggiornaStatoInMemoria(aid, dati);
    sw.checked = ['acceso', 'avvio'].includes(dati.stato);
    rigaStato.replaceChildren(puntoComputer(dati.stato, false), el('span', {}, [STATI_COMPUTER[dati.stato] || dati.stato,
      dati.larghezza && dati.altezza ? `${dati.larghezza}×${dati.altezza}` : null, dati.stato === 'acceso' ? daQuanto(dati.acceso_ms) : null].filter(Boolean).join(' · ')));
    errore.hidden = dati.stato !== 'errore';
    errore.textContent = dati.errore || '';
    clearTimeout(giro);
    if (dati.stato === 'avvio') giro = setTimeout(() => { if (sw.isConnected) api.computer(aid).then(scrivi).catch(() => {}); }, AVVIO_MS);
  }
  api.computer(aid).then(scrivi).catch((e) => { rigaStato.textContent = testoErrore(e); });
  return [
    el('div.campo-modulo', {}, el('span.riga-interruttore', {}, el('span', {}, 'Computer sulla VPS'), sw), rigaStato, errore),
    el('div.campo-modulo', {}, el('span', {}, 'Ricrea'), el('div.riga-tasti', {}, ricreaT), nota('Spegne il computer, cancella il suo profilo di Chrome e la sessione tmux, e lo riaccende pulito.')),
    nota('Il computer vive finché non lo spegni qui: chiudere lo schermo nel pannello chiude solo la vista. Si guarda dal pannello, scheda Computer.'),
  ];
}
