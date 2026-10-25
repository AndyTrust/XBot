// L'avvio di XBot: accesso, caricamenti, apertura di assistenti e sessioni, misure,
// scorciatoie da tastiera, service worker.

import { mostraAccesso } from './accesso.js';
import { api, aScadenzaAccesso, testoErrore } from './api.js';
import './api2d.js';
import './api2i.js';
import { apriFoglioAssistente } from './assistente.js';
import { avatar, avatarVivo, caricaPersonaggi } from './avatar.js';
import { disegnaBarra, preparaBarra, ricarica as ricaricaBarra } from './barra.js';
import { aggiornaColore, mostraTurno, preparaChat, segnaLetto, svuota } from './chat.js';
import { bannerPadre } from './figlie.js';
import { flusso, riprovaTutti } from './flusso.js';
import { iconaIn } from './icone.js';
import { apriEsporta, puoCondividere } from './esporta.js';
import { avviaFlussoStrumenti } from './flusso-strumenti.js';
import { apriImpostazioni } from './impostazioni.js';
import { avviaOrtografia } from './ortografia.js';
import { apriMenu, chiediConferma, sovrappostaAperta } from './menu.js';
import { disegnaPannello, preparaPannello } from './pannello.js';
import { apriRicerca } from './ricerca.js';
import { NOME_APP, usaNomeApp } from './nome.js';
import { avviaControlli, preparaVersione } from './aggiorna.js';
import { aggiornaBadge, dopoAccesso } from './push.js';
import { caricaMacchine, caricaScrittura, mettiFuoco, preparaScrittura } from './scrittura.js';
import { applicaAspetto, assistenteDi, bus, caricaAssistenti, caricaBarra, caricaProvider, caricaQuote, CHIAVI, impostazione, stato } from './stato.js';
import { $, avvisa, el, livelloIndietro, locale, quandoBreve } from './util.js';

const largo = () => matchMedia('(min-width: 1000px)').matches;
const stretto = () => matchMedia('(max-width: 699px)').matches;

applicaAspetto();
matchMedia('(prefers-color-scheme: dark)').addEventListener('change', applicaAspetto);

let preparato = false;
function preparaUnaVolta() {
  if (preparato) return;
  preparato = true;
  preparaChat();
  avviaFlussoStrumenti($('#messaggi'));
  avviaOrtografia();
  preparaScrittura({ apriUtilizzo: () => apriImpostazioni('utilizzo', { esci }), apriCassaforte: () => apriCassaforte(), apriComputer: () => apriImpostazioni('computer', { esci }) });
  preparaBarra({
    apri: (id) => (id ? apriAssistente(id) : mostraVuoto()),
    nuovo: () => nuovoAssistente(),
    cerca: () => cerca(),
    impostazioni: () => apriImpostazioni('generali', { esci }),
    collegamenti: () => apriImpostazioni('collegamenti', { esci }),
    squadra: () => apriImpostazioni('squadra', { esci }),
    impostazioniAssistente: (id) => impostazioniAssistente(id),
    nuovaSessione: (id) => nuovaSessione(id),
    esci: () => esci(),
  });
  preparaPannello({ chiudi: () => mostraPannello(false), impostazioniAssistente: (id) => impostazioniAssistente(id), apriCassaforte: (o) => apriCassaforte(o) });
  bus.su('barra', () => aggiornaBadge(stato.barra.flatMap((x) => x.assistenti || []).filter((v) => v.in_attesa_utente).length));
  bus.su('apri:sessione', ({ assistente_id: a, sessione_id: s }) => apriAssistente(a, s));
  navigator.serviceWorker?.addEventListener('message', (ev) => {
    if (ev.data?.tipo !== 'apri') return;
    if (ev.data.assistente_id) apriAssistente(ev.data.assistente_id, ev.data.sessione_id);
    else if (ev.data.sessione_id) api.sessione(ev.data.sessione_id).then((s) => apriAssistente(s.assistente_id, s.id)).catch(() => {});
  });
  iconaIn($('#tasto-cassetto'), 'menu', 20);
  iconaIn($('#tasto-info'), 'info', 20);
  iconaIn($('#pillola-menu'), 'giu', 14);
  $('#tasto-cassetto').onclick = () => cassetto(true);
  $('#velo').onclick = () => { cassetto(false); if (stretto()) mostraPannello(false); };
  $('#tasto-info').onclick = () => mostraPannello($('#pannello').hidden);
  $('#pillola').onclick = () => mostraPannello($('#pannello').hidden);
  $('#pillola-menu').onclick = (ev) => menuSessioni(ev.currentTarget);
  bus.su('flusso:turno', ({ sessioneId, stato: st }) => {
    ricaricaBarra();
    if (st !== 'in_corso') { caricaQuote(); if (sessioneId === stato.sessione?.id) mostraTurno(); }
  });
  bus.su('impostazioni', () => { aggiornaColore(); });
  bus.su('barra', () => { if (stato.assistenteId) { disegnaPillola(); disegnaVuotoChat(); } aggiornaColore(); });
  // dopo una rinomina o un cambio di colore anche la schermata vuota e la pillola si ridisegnano
  bus.su('assistenti', () => { if (stato.assistenteId) { disegnaPillola(); disegnaVuotoChat(); aggiornaColore(); } });
  document.addEventListener('keydown', tastiera);
  addEventListener('resize', () => { if (!stretto()) cassetto(false); });
}

async function avvia() {
  preparaUnaVolta();
  let io;
  try {
    io = await api.io();
  } catch (e) {
    if (e.codice === 'non_autorizzato') { mostraAccesso(avvia); return; }
    mostraAccesso(avvia, testoErrore(e));
    return;
  }
  stato.io = io;
  avviaControlli({ apri: () => apriImpostazioni('aggiornamenti', { esci }) });
  stato.impostazioni = io.impostazioni || {};
  applicaAspetto();
  $('#accesso').hidden = true;
  $('#finestra').hidden = false;
  try {
    await Promise.all([caricaProvider(), caricaAssistenti(), caricaBarra(), caricaQuote(), caricaPersonaggi()]);
  } catch (e) {
    avvisa(testoErrore(e), 4000);
  }
  caricaMacchine();
  disegnaBarra();
  // dal tocco su una notifica: /app/?assistente=…&sessione=…
  const qui = new URLSearchParams(location.search);
  const daIndirizzo = sezioneDaIndirizzo(qui);
  let daNotifica = qui.get('assistente');
  if (!daNotifica && qui.get('sessione')) { try { daNotifica = (await api.sessione(qui.get('sessione')))?.assistente_id || null; } catch { daNotifica = null; } }
  if (location.search || location.hash) history.replaceState(null, '', location.pathname);
  const ultimo = locale.leggi('ultimo_assistente');
  const primo = stato.barra.flatMap((s) => s.assistenti || [])[0]?.assistente_id;
  const id = stato.assistenti.has(daNotifica) ? daNotifica : stato.assistenti.has(ultimo) ? ultimo : primo;
  if (id) await apriAssistente(id, daNotifica === id ? qui.get('sessione') : undefined); else mostraVuoto();
  dopoAccesso();
  if (largo()) mostraPannello(impostazione(CHIAVI.pannello, true) !== false && !!id);
  riprovaTutti();
  if (daIndirizzo) apriImpostazioni(daIndirizzo.sezione, { esci, opzioni: daIndirizzo.opzioni });
}

/** Ritorno dal consenso OAuth (CONTRATTO-2I 2.1): `#impostazioni/collegamenti?oauth=ok|errore&collegamento=…&motivo=…`,
 * o gli stessi parametri nella query. Restituisce {sezione, opzioni} o null. */
function sezioneDaIndirizzo(qui) {
  const m = location.hash.match(/^#impostazioni\/([a-z]+)(?:\?(.*))?$/);
  const p = m ? new URLSearchParams(m[2] || '') : qui;
  const sezione = m ? m[1] : (qui.get('oauth') ? 'collegamenti' : null);
  if (!sezione) return null;
  const opzioni = p.get('oauth') ? { oauth: p.get('oauth'), collegamento: p.get('collegamento'), motivo: p.get('motivo') } : {};
  return { sezione, opzioni };
}

aScadenzaAccesso(() => {
  flusso.chiudi();
  if (!$('#accesso').hidden) return; // la pagina di accesso è già aperta: non si copre il suo errore
  mostraAccesso(avvia, 'L\'accesso è scaduto: entra di nuovo.');
});

async function esci() {
  try { await api.esci(); } catch { /* si esce comunque */ }
  flusso.chiudi();
  document.getElementById('sovrapposte').replaceChildren();
  mostraAccesso(avvia);
}

// ---------- Assistenti e sessioni ----------

async function apriAssistente(id, sessioneId) {
  stato.assistenteId = id;
  locale.scrivi('ultimo_assistente', id);
  cassetto(false);
  let sessioni = [];
  try { sessioni = await api.sessioni(id); } catch (e) { avvisa(testoErrore(e)); }
  if (stato.assistenteId !== id) return;
  stato.sessioni = sessioni.sort((a, b) => b.aggiornata_ms - a.aggiornata_ms);
  const voluta = sessioneId || locale.leggi(`ultima_sessione.${id}`);
  let s = stato.sessioni.find((x) => x.id === voluta) || stato.sessioni.find((x) => !x.sessione_padre_id) || stato.sessioni[0];
  if (!s) {
    try { s = await api.creaSessione(id, {}); stato.sessioni = [s]; } catch (e) { avvisa(testoErrore(e)); return; }
  }
  apriSessione(s);
}

function apriSessione(s) {
  stato.sessione = s;
  stato.turno = null;
  locale.scrivi(`ultima_sessione.${stato.assistenteId}`, s.id);
  $('#vuoto').hidden = true;
  $('#scrittura').hidden = false;
  svuota();
  mostraTurno();
  disegnaPillola();
  caricaScrittura();
  disegnaBarra();
  if (!$('#pannello').hidden) disegnaPannello();
  disegnaVuotoChat();
  flusso.apri(s.id);
  segnaLetto();
  bus.emetti('sessione');
  if (!stretto()) mettiFuoco();
}

function disegnaPillola() {
  const a = assistenteDi(stato.assistenteId);
  const v = stato.barra.flatMap((x) => x.assistenti || []).find((x) => x.assistente_id === stato.assistenteId) || {};
  $('#pillola').replaceChildren(avatarVivo(a, 32, { voce: v }), el('span', {}, a.nome || ''));
  document.querySelector('.torna-padre')?.remove();
  const padre = bannerPadre(stato.sessione);
  if (padre) $('#pillola').after(padre);
  $('#pillola').setAttribute('aria-label', `${a.nome}: dettagli`);
  document.title = a.nome ? `${a.nome} · ${NOME_APP}` : NOME_APP;
}

function disegnaVuotoChat() {
  const a = assistenteDi(stato.assistenteId);
  $('#vuoto').replaceChildren(el('div', {}, avatar(a, 64, { stato: false }), el('h2', {}, a.nome), el('p', {}, `Scrivi il primo messaggio a ${a.nome}.`)));
}

function mostraVuoto() {
  flusso.chiudi();
  stato.assistenteId = null;
  stato.sessione = null;
  svuota();
  $('#pillola').replaceChildren();
  $('#scrittura').hidden = true;
  mostraPannello(false);
  $('#vuoto').replaceChildren(el('div', {}, el('h2', {}, 'Nessun assistente aperto'),
    el('p', {}, 'Crea un assistente per cominciare.'),
    el('button.tasto.primario', { type: 'button', onclick: () => nuovoAssistente() }, 'Nuovo assistente')));
  $('#vuoto').hidden = false;
}

function nuovoAssistente(nome = '') {
  apriFoglioAssistente(null, { nomeIniziale: nome, dopo: (id) => (id ? apriAssistente(id) : null) });
}

function impostazioniAssistente(id, scheda) {
  apriFoglioAssistente(id, { scheda, apriCassaforte: (o) => apriCassaforte(o), dopo: (nuovo) => { if (!nuovo && id === stato.assistenteId) mostraVuoto(); } });
}

function apriCassaforte(opzioni) {
  apriImpostazioni('cassaforte', { esci, opzioni });
}

async function nuovaSessione(id = stato.assistenteId) {
  try {
    const s = await api.creaSessione(id, {});
    if (id !== stato.assistenteId) { await apriAssistente(id, s.id); return; }
    stato.sessioni.unshift(s);
    apriSessione(s);
  } catch (e) { avvisa(testoErrore(e)); }
}

function cerca() {
  if (sovrappostaAperta()) return;
  apriRicerca({
    apri: (id, sid) => apriAssistente(id, sid),
    nuovo: (nome) => nuovoAssistente(nome),
    impostazioni: () => apriImpostazioni('generali', { esci }),
  });
}

function menuSessioni(ancora) {
  if (!stato.assistenteId) return;
  const a = assistenteDi(stato.assistenteId);
  apriMenu(ancora, [
    { titolo: `Sessioni di ${a.nome}` },
    ...stato.sessioni.slice(0, 12).map((s, i) => ({
      testo: `${s.sessione_padre_id ? '↳ ' : ''}${s.titolo || `Sessione ${stato.sessioni.length - i}`}`, nota: `${quandoBreve(s.aggiornata_ms)} · ${s.macchina_id || 'vps'}`,
      icona: s.id === stato.sessione?.id ? 'spunta' : 'sessioni', azione: () => apriSessione(s),
    })),
    '-',
    { testo: 'Nuova sessione', icona: 'piu', azione: () => nuovaSessione() },
    { testo: 'Azzera conversazione…', icona: 'chiudi', disattivo: !stato.sessione, azione: async () => {
      const ok = await chiediConferma({ titolo: 'Azzerare la conversazione?', testo: `I messaggi con ${a.nome} spariscono da questa sessione e si riparte da capo.`, conferma: 'Azzera', pericolo: true });
      if (!ok) return;
      try { await api.azzera(stato.sessione.id); avvisa('Conversazione azzerata'); } catch (e) { avvisa(testoErrore(e)); }
    } },
    { testo: 'Esporta…', icona: 'scarica', disattivo: !stato.sessione, azione: () => apriEsporta() },
    puoCondividere() ? { testo: 'Condividi…', icona: 'carica', disattivo: !stato.sessione, azione: () => apriEsporta({ condividi: true }) } : null,
    { testo: 'Archivia questa sessione', icona: 'archivia', disattivo: !stato.sessione, azione: async () => {
      try {
        await api.modificaSessione(stato.sessione.id, { archivia: true });
        locale.scrivi(`ultima_sessione.${stato.assistenteId}`, null);
        apriAssistente(stato.assistenteId);
      } catch (e) { avvisa(testoErrore(e)); }
    } },
  ].filter(Boolean));
}

// ---------- Misure: pannello, cassetto, foglio ----------

let togliPannello = null;
let togliCassetto = null;

function mostraPannello(si) {
  const p = $('#pannello');
  if (si && !stato.assistenteId) si = false;
  p.hidden = !si;
  // sul telefono il foglio del pannello e il cassetto si chiudono con Indietro di Android
  if (si && stretto() && !togliPannello) togliPannello = livelloIndietro(() => { togliPannello = null; mostraPannello(false); });
  else if (!si && togliPannello) { const t = togliPannello; togliPannello = null; t(); }
  $('#finestra').classList.toggle('con-pannello', si);
  $('#velo').hidden = !(si && stretto()) && !$('#finestra').classList.contains('cassetto-aperto');
  if (si) disegnaPannello();
  if (largo()) locale.scrivi('pannello', si);
}

function cassetto(si) {
  $('#finestra').classList.toggle('cassetto-aperto', si && stretto());
  if (si && stretto() && !togliCassetto) togliCassetto = livelloIndietro(() => { togliCassetto = null; cassetto(false); });
  else if (!si && togliCassetto) { const t = togliCassetto; togliCassetto = null; t(); }
  $('#velo').hidden = !(si && stretto()) && !(stretto() && !$('#pannello').hidden);
  if (si) $('#sezioni .voce, #fissati .fissato')?.focus();
}

// ---------- Tastiera ----------

function tastiera(ev) {
  if ($('#finestra').hidden) return;
  const mod = ev.metaKey || ev.ctrlKey;
  if (mod && ev.key.toLowerCase() === 'k') { ev.preventDefault(); cerca(); return; }
  if (sovrappostaAperta()) return;
  if (mod && ev.key.toLowerCase() === 'i') { ev.preventDefault(); mostraPannello($('#pannello').hidden); return; }
  if (ev.altKey && ev.code === 'KeyN') { ev.preventDefault(); nuovoAssistente(); return; }
  if (mod && /^[1-9]$/.test(ev.key)) {
    const fissati = stato.barra.flatMap((s) => s.assistenti || []).filter((v) => v.fissato);
    const v = fissati[Number(ev.key) - 1];
    if (v) { ev.preventDefault(); apriAssistente(v.assistente_id); }
    return;
  }
  if (ev.key === 'Escape') { cassetto(false); if (stretto()) mostraPannello(false); }
}

// ---------- Service worker (cache dei file statici, mai l'API) ----------

// Nome della app e versione installata da GET /salute (senza accesso, una volta sola): la versione
// è la chiave della cache del service worker (aggiorna.js).
preparaVersione().then((s) => usaNomeApp(s?.nome_app));
avvia();
