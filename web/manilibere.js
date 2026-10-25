// Chat vocale (richiesta del 2026-10-24 21:25: pannello flottante con trascrizione), al posto del vecchio schermo
// pieno «mani libere». Pannello flottante in alto sopra la chat: avatar che si anima mentre
// l'assistente parla, onda a barrette nel suo colore (volume del microfono quando ascolta, della
// sintesi quando parla, puntini a riposo), tasti impostazioni · trascrizione · silenzia · riaggancia;
// sotto, la trascrizione dal vivo. Il giro resta quello di prima: ascolta (1,2 s di silenzio),
// trascrive, invia, aspetta la fine del turno, legge con la voce dell'assistente, ricomincia.
// Schermo tenuto acceso con wakeLock; con la pagina nascosta si riaggancia. A fine chiamata
// in chat resta la riga «Chat vocale · mm:ss» (chat.js, evento `chiamata:finita`).

import { api, testoErrore } from './api.js';
import { apriFoglioAssistente } from './assistente.js';
import { avatarSvg, coloreDi, fotoDi } from './avatar.js';
import { invia } from './flusso.js';
import { icona } from './icone.js';
import { apriMenu } from './menu.js';
import { assistenteDi, bus, caricaAssistenti, impostazione, scriviImpostazione, stato } from './stato.js';
import { $, avvisa, el, locale, ulid } from './util.js';
import { classifica, eEco } from './smistatore.js';
import { apriMicrofono, CHIAVE_PARLATA, lettoreVoce, parla, registratore, staParlando, testoParlabile, trascrivi, zitto } from './voce.js';

const SILENZIO_MS = 1200;
const PARLATO_MIN_MS = 250;
// Mentre l'assistente parla il microfono resta in ascolto, ma con una soglia più alta (l'eco che l'AEC
// non toglie del tutto) e un parlato minimo più lungo; la soglia sale anche col volume della sintesi.
const SOGLIA_PARLO = 0.05;
const PARLATO_MIN_PARLO_MS = 350;
const PREROLL_PARLO_MS = 1200;
const QUOTA_VOCE = 0.25;
// L'AEC del telefono impiega qualche secondo a convergere a ogni lettura: all'inizio la soglia parte alta
// (eco forte, misurato sul S24: rms 0,22 a 1,7 s) e scende fino alla soglia di base in RAMPA_MS.
const SOGLIA_INIZIO = 0.25;
const RAMPA_MS = 4000;
const ATTESA_MAX_MS = 10 * 60 * 1000;
const BARRE = 20;
const STATI = { preparo: 'Preparo il microfono', ascolto: 'Ti ascolto', trascrivo: 'Trascrivo', attesa: 'Sta pensando', parlo: 'Ti risponde', muto: 'Microfono silenziato' };

let aperta = null;

// Un solo AudioContext per la pagina: l'elemento audio della sintesi si può collegare una volta sola
// (createMediaElementSource), e da allora suona attraverso il contesto, che quindi non si chiude mai.
let audio = null;
function contestoAudio() {
  if (!audio) {
    const ctx = new (window.AudioContext || window.webkitAudioContext)();
    const voce = ctx.createAnalyser();
    voce.fftSize = 512;
    try { const src = ctx.createMediaElementSource(lettoreVoce()); src.connect(voce); voce.connect(ctx.destination); } catch { /* già collegato */ }
    audio = { ctx, voce };
  }
  audio.ctx.resume?.().catch(() => {});
  return audio;
}

function livello(an, dati) {
  an.getFloatTimeDomainData(dati);
  let somma = 0;
  for (const v of dati) somma += v * v;
  return Math.sqrt(somma / dati.length);
}

export function apriManiLibere() {
  if (aperta || !stato.sessione) return;
  const a = assistenteDi(stato.assistenteId);
  const foto = !!fotoDi(a);
  const viso = el(`span.ch-avatar${foto ? '.foto' : ''}`, { 'aria-hidden': 'true' }, avatarSvg(a, 40));
  const barre = Array.from({ length: BARRE }, () => el('span'));
  const onda = el('button.ch-onda', { type: 'button', 'aria-label': 'Interrompi', title: 'Interrompi', onclick: () => interrompiTutto() }, barre);
  const tasto = (ic, etichetta, onclick, classe = '') => el(`button.ch-tasto${classe}`, { type: 'button', 'aria-label': etichetta, title: etichetta, onclick }, icona(ic, 18));
  const trascrizione = el('div.ch-trascrizione', { 'aria-live': 'polite', 'aria-label': 'Trascrizione' });
  const fase = el('span.ch-fase', { role: 'status' });
  const tChat = tasto('fumetto', 'Trascrizione', () => mostraTrascrizione(s, s.nodo.classList.contains('senza-trascrizione')));
  const tMic = tasto('microfono', 'Silenzia', () => silenzia(s));
  const nodo = el('div.chiamata', { role: 'dialog', 'aria-label': `Chat vocale con ${a.nome}`, style: `--colore-chiamata:${coloreDi(a)}` },
    el('div.ch-pillola', {}, viso, onda,
      el('span.ch-tasti', {}, tasto('impostazioni', 'Impostazioni della voce', (ev) => menuVoce(ev.currentTarget, a.id)), tChat, tMic,
        tasto('chiudi', 'Riaggancia', () => ferma(), '.riaggancia'))),
    trascrizione, fase);
  ($('#chat') || document.body).append(nodo);
  const s = {
    nodo, viso, barre, onda, fase, trascrizione, tChat, tMic, foto, a, vivo: true, flusso: null, sorgente: null, analizzatore: null, rec: null,
    giro: 0, wake: null, turno: null, turniPresi: new Set(), turniAttivi: new Set(), attese: 0, epoca: 0, trascrivendo: 0, ultimaParlata: '', aec: true, fase_: '', muto: false, inizio: Date.now(), scambi: 0, invii: 0, sessioneId: stato.sessione.id, bollaRisposta: null,
  };
  aperta = s;
  bus.manilibere = true;
  mostraTrascrizione(s, locale.leggi('chiamata.trascrizione', true) !== false);
  segna(s, 'preparo');
  document.addEventListener('visibilitychange', nascosta);
  s.togliRighe = bus.su('flusso:righe', ({ sessioneId, righe }) => {
    if (sessioneId !== s.sessioneId || !s.turno) return;
    for (const r of righe) if (r.genere === 'messaggio' && r.ruolo === 'assistente' && r.corpo?.turno_id === s.turno) rispostaDalVivo(s, r.corpo.testo || '');
  });
  anima(s);
  avvia(s).catch((e) => { segna(s, 'preparo'); s.fase.textContent = e.message || String(e); bolla(s, 'sistema', e.message || String(e)); });
}

function segna(s, f) {
  s.fase_ = f;
  s.nodo.dataset.fase = f;
  s.fase.textContent = s.muto && f === 'ascolto' ? STATI.muto : STATI[f] || f;
  s.onda.disabled = !['attesa', 'parlo', 'trascrivo'].includes(f);
}
function nascosta() { if (document.visibilityState === 'hidden') ferma(); }

/** Una bolla nella trascrizione: l'utente a destra scura, l'assistente a sinistra chiara. */
function bolla(s, chi, testo) {
  const b = el(`div.ch-bolla.${chi}`, {}, testo);
  s.trascrizione.append(b);
  s.trascrizione.scrollTop = s.trascrizione.scrollHeight;
  return b;
}

function rispostaDalVivo(s, testo) {
  const t = testoParlabile(testo) || testo;
  if (!t) return;
  if (!s.bollaRisposta) s.bollaRisposta = bolla(s, 'assistente', t);
  else s.bollaRisposta.textContent = t;
  s.trascrizione.scrollTop = s.trascrizione.scrollHeight;
}

function mostraTrascrizione(s, si) {
  s.nodo.classList.toggle('senza-trascrizione', !si);
  s.tChat.setAttribute('aria-pressed', si ? 'true' : 'false');
  s.tChat.title = si ? 'Nascondi la trascrizione' : 'Mostra la trascrizione';
  locale.scrivi('chiamata.trascrizione', si);
}

function silenzia(s) {
  s.muto = !s.muto;
  s.flusso?.getAudioTracks().forEach((t) => { t.enabled = !s.muto; });
  s.tMic.replaceChildren(icona(s.muto ? 'micmuto' : 'microfono', 18));
  s.tMic.setAttribute('aria-pressed', s.muto ? 'true' : 'false');
  s.tMic.title = s.muto ? 'Riattiva' : 'Silenzia';
  s.tMic.setAttribute('aria-label', s.muto ? 'Riattiva il microfono' : 'Silenzia');
  s.tMic.classList.toggle('acceso', s.muto);
  segna(s, s.fase_);
}

function menuVoce(ancora, aid) {
  const a = assistenteDi(aid);
  const vel = Number(a.voce_velocita || impostazione('interfaccia.voce_velocita', 1)) || 1;
  const salvaA = async (campi, detto) => {
    try { await api.modificaAssistente(aid, campi); await caricaAssistenti(); avvisa(detto); } catch (e) { avvisa(testoErrore(e)); }
  };
  const parlata = impostazione(CHIAVE_PARLATA, false) === true;
  apriMenu(ancora, [
    { titolo: 'Velocità della voce' },
    ...[0.9, 1, 1.1, 1.25].map((v) => ({ testo: `${String(v).replace('.', ',')}×`, icona: Math.abs(v - vel) < 0.01 ? 'spunta' : null, azione: () => salvaA({ voce_velocita: v }, `Velocità ${String(v).replace('.', ',')}×`) })),
    { titolo: 'Lingua del dettato e della voce' },
    ...[['it', 'Italiano'], ['en', 'Inglese']].map(([k, t]) => ({ testo: t, icona: (a.voce_lingua || 'it') === k ? 'spunta' : null, azione: () => salvaA({ voce_lingua: k }, t) })),
    '-',
    { testo: parlata ? 'Risposta parlata in chat: accesa' : 'Risposta parlata in chat: spenta', icona: 'altoparlante', azione: () => scriviImpostazione(CHIAVE_PARLATA, !parlata).then(() => avvisa(parlata ? 'Risposta parlata spenta' : 'Risposta parlata accesa')).catch((e) => avvisa(testoErrore(e))) },
    { testo: 'Voce e altre impostazioni…', icona: 'impostazioni', azione: () => apriFoglioAssistente(aid, { scheda: 'modello' }) },
  ]);
}

async function avvia(s) {
  try { s.wake = await navigator.wakeLock?.request('screen'); } catch { s.wake = null; }
  const { ctx } = contestoAudio();
  s.flusso = await apriMicrofono();
  const imp = s.flusso.getAudioTracks()[0]?.getSettings?.() || {};
  s.aec = imp.echoCancellation !== false; // senza cancellazione dell'eco la soglia in riproduzione raddoppia
  bus.emetti('voce:microfono', { echoCancellation: imp.echoCancellation, noiseSuppression: imp.noiseSuppression, autoGainControl: imp.autoGainControl });
  if (!s.vivo) { s.flusso.getTracks().forEach((t) => t.stop()); return; }
  s.sorgente = ctx.createMediaStreamSource(s.flusso);
  s.analizzatore = ctx.createAnalyser();
  s.analizzatore.fftSize = 1024;
  s.sorgente.connect(s.analizzatore);
  giro(s);
}

/** Il giro d'ascolto: ascolta → (in disparte) trascrive e smista. Non aspetta niente: il microfono resta sempre vivo. */
async function giro(s) {
  const mio = ++s.giro;
  const vale = () => s.vivo && s.giro === mio;
  while (vale()) {
    const registrato = await ascolta(s, vale);
    if (!vale() || !registrato) return;
    elabora(s, registrato, vale);
  }
}

async function elabora(s, registrato, vale) {
  s.trascrivendo++;
  segna(s, 'trascrivo');
  const mia = bolla(s, 'utente', '…');
  let detto = '';
  try { detto = await trascrivi(registrato); } catch (e) { mia.textContent = `Trascrizione non riuscita: ${testoErrore(e)}`; mia.classList.add('errore'); return; } finally { s.trascrivendo--; }
  if (!vale()) return;
  if (!detto || eEco(detto, s.ultimaParlata)) { mia.remove(); return; }
  mia.textContent = detto;
  smista(s, detto, vale);
}

/** Decide dove va ciò che l'utente ha detto: stessa sessione, assistente in parallelo, oppure «basta». */
function smista(s, detto, vale) {
  const c = classifica(detto, [...stato.assistenti.values()], s.a.id);
  if (c.tipo === 'ferma') { interrompiTutto(); return; }
  if (c.tipo === 'parallelo') { parallelo(s, c, vale).catch((e) => { bolla(s, 'sistema', testoErrore(e)); }); return; }
  consegna(s, c.testo, vale);
}

/** Continuazione: invio nella sessione in corso (se il turno è ancora in corso va in coda), poi si legge la risposta. */
async function consegna(s, detto, vale) {
  const epoca = s.epoca;
  const ok = () => vale() && s.epoca === epoca;
  s.attese++;
  s.invii++;
  s.bollaRisposta = null;
  let risposta = '';
  try { risposta = await inviaEAspetta(s, detto, ok); } finally { s.attese--; }
  if (!ok()) return;
  if (!risposta) { rispostaDalVivo(s, 'Nessuna risposta.'); return; }
  s.scambi++;
  rispostaDalVivo(s, risposta);
  s.ultimaParlata = testoParlabile(risposta);
  await parla(risposta);
}

/** Richiesta nuova da affidare a un altro assistente (o a una sessione nuova dello stesso) senza fermare questa. */
async function parallelo(s, c, vale) {
  const altro = c.assistente || s.a;
  const nome = altro.nome;
  const frase = c.assistente ? `Lo affido a ${nome}, intanto qui continuo.` : 'Apro una sessione nuova per questo, intanto qui continuo.';
  bolla(s, 'sistema', `${frase} (${c.testo})`);
  s.ultimaParlata = frase;
  parla(frase, s.a.id);
  // Chat figlia di questa (piano sessioni annidate): il padre vede stato e risultato, la richiesta parte dal server.
  const f = await api.apriFiglia(stato.sessione.id, { compito: c.testo, menzione: c.assistente ? `@${c.assistente.menzione}` : '', titolo: c.testo.slice(0, 60) });
  const nuova = { id: f.figlia_id };
  s.invii++;
  bus.emetti('sessioni:cambiate', { assistenteId: altro.id });
  // La risposta dell'altro non passa dal flusso di questa sessione: si legge a intervalli finché il testo non cambia più.
  let ultimo = '';
  let fermo = 0;
  for (let i = 0; i < 100 && vale(); i++) {
    await pausa(3000);
    let r;
    try { r = await api.righe(nuova.id, 0); } catch { continue; }
    const m = (r?.righe || []).filter((x) => x.genere === 'messaggio' && x.ruolo === 'assistente' && x.corpo?.testo).pop();
    const t = m?.corpo?.testo || '';
    if (t && t === ultimo) fermo++; else fermo = 0;
    ultimo = t;
    if (t && fermo >= 1) {
      const breve = testoParlabile(t).slice(0, 700);
      bolla(s, 'assistente', `${nome}: ${breve}`);
      if (vale()) { s.ultimaParlata = breve; await parla(`${nome} risponde. ${breve}`, altro.id); }
      return;
    }
  }
}

const pausa = (ms) => new Promise((r) => setTimeout(r, ms));

/** Registra finché parli, e chiude dopo 1,2 s di silenzio. Resta in ascolto anche mentre l'assistente parla: se l'utente
 * supera la soglia alta, la voce si ferma subito (barge-in) e si continua a registrare. */
function ascolta(s, vale) {
  return new Promise((risolvi) => {
    const r = registratore(s.flusso);
    s.rec = r;
    r.avvia();
    const dati = new Float32Array(s.analizzatore.fftSize);
    const datiVoce = new Float32Array(audio.voce.fftSize);
    let fondo = 0.01;
    let parlatoDa = 0;
    let ultimoSuono = 0;
    let parlato = false;
    let avviata = performance.now();
    let eraParlando = false;
    let inizioParlo = 0;
    let lvLiscio = 0;
    const passo = async () => {
      if (!vale()) { await r.ferma(); risolvi(null); return; }
      const parlando = staParlando();
      if (!s.trascrivendo) {
        const f = parlando ? 'parlo' : s.attese ? 'attesa' : 'ascolto';
        if (f !== s.fase_) segna(s, f);
      }
      s.analizzatore.getFloatTimeDomainData(dati);
      let somma = 0;
      for (const v of dati) somma += v * v;
      const rms = s.muto ? 0 : Math.sqrt(somma / dati.length);
      const ora = performance.now();
      let soglia = Math.max(0.018, fondo * 3);
      if (audio.ctx.state !== 'running') audio.ctx.resume?.().catch(() => {}); // Android lo sospende dopo un'interruzione audio
      if (s.flusso.getAudioTracks().every((x) => x.readyState === 'ended')) { riapriMicrofono(s); risolvi(null); return; }
      if (parlando) {
        if (!eraParlando) { inizioParlo = ora; lvLiscio = 0; }
        lvLiscio = lvLiscio * 0.9 + livello(audio.voce, datiVoce) * 0.1;
        const base = Math.max(SOGLIA_PARLO * (s.aec ? 1 : 2), fondo * 4, lvLiscio * QUOTA_VOCE);
        soglia = base + Math.max(0, SOGLIA_INIZIO - base) * Math.max(0, 1 - (ora - inizioParlo) / RAMPA_MS);
      }
      eraParlando = parlando;
      if (rms > soglia) {
        if (!parlatoDa) parlatoDa = ora;
        if (ora - parlatoDa > (parlando ? PARLATO_MIN_PARLO_MS : PARLATO_MIN_MS)) {
          if (!parlato && parlando) { zitto(); s.bollaRisposta = null; bus.emetti('voce:interrotta', { rms, soglia }); } // barge-in
          parlato = true;
        }
        ultimoSuono = ora;
      } else {
        parlatoDa = 0;
        if (!parlando) fondo = fondo * 0.95 + rms * 0.05;
      }
      if (parlato && ora - ultimoSuono > SILENZIO_MS) { risolvi(await r.ferma()); return; }
      // niente voce: si butta e si riparte (a ogni 1,2 s mentre l'assistente parla, per non registrare la sua eco)
      if (!parlato && ora - avviata > (parlando ? PREROLL_PARLO_MS : 30000)) { await r.ferma(); r.avvia(); avviata = ora; }
      setTimeout(passo, 50);
    };
    passo();
  });
}

/** La traccia del microfono è finita (un'altra app lo ha preso, o il sistema lo ha tolto): si riapre e si riparte. */
async function riapriMicrofono(s) {
  try {
    s.flusso?.getTracks().forEach((x) => x.stop());
    try { s.sorgente?.disconnect(); } catch { /* già staccato */ }
    s.flusso = await apriMicrofono();
    s.sorgente = audio.ctx.createMediaStreamSource(s.flusso);
    s.sorgente.connect(s.analizzatore);
    s.s_riaperto = (s.s_riaperto || 0) + 1;
    bus.emetti('voce:microfono', { riaperto: s.s_riaperto });
    if (s.vivo) giro(s);
  } catch (e) { bolla(s, 'sistema', `Microfono perso: ${e.message || e}`); ferma(); }
}

/** Invia il testo detto e aspetta la fine del turno; restituisce il testo della risposta.
 * Se il turno è in coda dietro un altro, il suo id si conosce solo quando parte: si prende il primo turno nuovo. */
function inviaEAspetta(s, detto, vale) {
  return new Promise((risolvi) => {
    const sessione = stato.sessione.id;
    let turnoId = null;
    let fatto = false;
    const prendi = (id) => { turnoId = id; s.turniPresi.add(id); s.turniAttivi.add(id); s.turno = id; };
    const finisci = (t) => { if (fatto) return; fatto = true; togliEsito(); togliTurno(); clearTimeout(limite); clearInterval(controlla); if (turnoId) s.turniAttivi.delete(turnoId); risolvi(t); };
    const voce = invia(sessione, detto, { origine: 'voce' });
    const togliEsito = bus.su('invio:esito', ({ voce: v, esito }) => { if (v.id === voce.id && esito?.turno_id && !turnoId) prendi(esito.turno_id); });
    const togliTurno = bus.su('flusso:turno', ({ sessioneId, turno_id, stato: st }) => {
      if (sessioneId !== sessione) return;
      if (!turnoId) { if (st === 'in_corso' && !s.turniPresi.has(turno_id)) prendi(turno_id); return; }
      if (st === 'in_corso' || turno_id !== turnoId) return;
      setTimeout(() => finisci(st === 'riuscito' ? bus.testoDelTurno?.(turno_id) || '' : ''), 300);
    });
    const limite = setTimeout(() => finisci(''), ATTESA_MAX_MS);
    const controlla = setInterval(() => { if (!vale()) finisci(''); }, 300);
  });
}

/** Onda e avatar: ogni fotogramma legge il volume del microfono (ascolto) o della sintesi (parlo). */
function anima(s) {
  const dati = new Float32Array(1024);
  let variante = 0;
  let cambio = 0;
  const passo = (t) => {
    if (!s.vivo) return;
    let v = 0;
    if (s.fase_ === 'ascolto' && !s.muto && s.analizzatore) v = livello(s.analizzatore, dati);
    else if (s.fase_ === 'parlo' && audio) v = livello(audio.voce, dati.subarray(0, audio.voce.fftSize));
    const forte = Math.min(1, v * 9);
    s.barre.forEach((b, i) => {
      const forma = 0.45 + 0.55 * Math.sin((i / (BARRE - 1)) * Math.PI); // più alte al centro
      const h = forte < 0.04 ? 4 : 4 + forte * 26 * forma * (0.6 + 0.4 * Math.abs(Math.sin(t / 90 + i * 1.7)));
      b.style.height = `${h.toFixed(1)}px`;
    });
    s.onda.classList.toggle('riposo', forte < 0.04);
    // l'avatar parla: la foto pulsa col volume, le forme alternano tre espressioni a ritmo dell'audio
    const parla_ = s.fase_ === 'parlo' && forte > 0.05;
    if (s.foto) s.viso.style.transform = parla_ ? `scale(${(1 + forte * 0.08).toFixed(3)})` : '';
    else if (parla_ && t - cambio > 140 + (1 - forte) * 200) { variante = (variante + 1) % 3; cambio = t; s.viso.dataset.variante = String(variante); }
    else if (!parla_ && s.viso.dataset.variante !== '0') s.viso.dataset.variante = '0';
    s.viso.classList.toggle('parla', parla_);
    requestAnimationFrame(passo);
  };
  requestAnimationFrame(passo);
}

/** «Interrompi» / «basta»: zittisce la voce, ferma i turni in corso; il microfono resta in ascolto. */
async function interrompiTutto() {
  const s = aperta;
  if (!s) return;
  zitto();
  s.epoca++;
  const turni = [...s.turniAttivi];
  s.turniAttivi.clear();
  s.attese = 0;
  for (const t of turni) { try { await api.interrompi(t); } catch { /* già finito */ } }
  s.bollaRisposta = null;
  if (!s.analizzatore) ferma();
}

/** Riaggancia: microfono, voce, schermo acceso; in chat la riga della chiamata. */
export function ferma() {
  const s = aperta;
  if (!s) return;
  aperta = null;
  s.vivo = false;
  bus.manilibere = false;
  document.removeEventListener('visibilitychange', nascosta);
  zitto();
  try { s.rec?.rec.state !== 'inactive' && s.rec?.rec.stop(); } catch { /* già fermo */ }
  s.togliRighe?.();
  s.flusso?.getTracks().forEach((t) => t.stop());
  try { s.sorgente?.disconnect(); } catch { /* già staccato */ }
  s.wake?.release?.().catch(() => {});
  s.nodo.remove();
  const durata = Date.now() - s.inizio;
  if (s.invii > 0 || durata > 3000) {
    bus.emetti('chiamata:finita', { id: ulid(), sessioneId: s.sessioneId, iniziata_ms: s.inizio, durata_ms: durata, invii: s.invii, risposte: s.scambi, fine_ms: Date.now() });
  }
}

document.addEventListener('keydown', (ev) => { if (ev.key === 'Escape' && aperta) { ev.preventDefault(); ev.stopPropagation(); ferma(); } }, true);
