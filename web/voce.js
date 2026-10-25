// La voce (CONTRATTO-2D sez. 7): microfono (permesso chiesto una volta), registrazione con
// MediaRecorder (webm/opus, in ripiego mp4), trascrizione (POST /trascrivi), sintesi
// (POST /sintesi) con una coda di riproduzione, risposta parlata a fine turno.

import { api, testoErrore } from './api.js';
import { assistenteDi, bus, impostazione, stato } from './stato.js';
import { avvisa } from './util.js';

export const CHIAVE_PARLATA = 'interfaccia.risposta_parlata';
export const CHIAVE_MICROFONO = 'interfaccia.microfono';

// ---------- Microfono ----------
/** Apre il microfono. Lancia un Error con un testo chiaro se manca o è negato. */
export async function apriMicrofono() {
  if (!navigator.mediaDevices?.getUserMedia) throw new Error('Questo browser non dà il microfono (serve HTTPS).');
  try {
    const p = await navigator.permissions?.query({ name: 'microphone' });
    if (p?.state === 'denied') throw new Error('negato');
  } catch (e) { if (e.message === 'negato') throw new Error('Il microfono è negato: riattivalo dalle impostazioni del sito nel browser, poi riprova.'); }
  const dispositivo = impostazione(CHIAVE_MICROFONO, '');
  try {
    return await navigator.mediaDevices.getUserMedia({ audio: dispositivo ? { deviceId: { exact: dispositivo }, echoCancellation: true, noiseSuppression: true } : { echoCancellation: true, noiseSuppression: true, autoGainControl: true } });
  } catch (e) {
    if (e.name === 'NotAllowedError' || e.name === 'SecurityError') {
      throw new Error('Il microfono è negato: riattivalo dalle impostazioni del sito nel browser, poi riprova.');
    }
    if (e.name === 'OverconstrainedError' && dispositivo) return navigator.mediaDevices.getUserMedia({ audio: true });
    throw new Error(e.name === 'NotFoundError' ? 'Nessun microfono trovato.' : `Microfono non disponibile (${e.name}).`);
  }
}

/** Il formato che il browser sa registrare: webm/opus, poi mp4. */
export function formatoRegistrazione() {
  for (const t of ['audio/webm;codecs=opus', 'audio/webm', 'audio/mp4', 'audio/ogg;codecs=opus']) {
    if (window.MediaRecorder?.isTypeSupported?.(t)) return t;
  }
  return '';
}

/** Un registratore su un flusso: start(), stop() → Blob. */
export function registratore(flussoAudio) {
  const tipo = formatoRegistrazione();
  const rec = new MediaRecorder(flussoAudio, tipo ? { mimeType: tipo } : undefined);
  const pezzi = [];
  rec.ondataavailable = (ev) => { if (ev.data?.size) pezzi.push(ev.data); };
  return {
    rec,
    avvia() { pezzi.length = 0; rec.start(250); },
    ferma() {
      return new Promise((risolvi) => {
        if (rec.state === 'inactive') { risolvi(new Blob(pezzi, { type: rec.mimeType || tipo })); return; }
        rec.onstop = () => risolvi(new Blob(pezzi, { type: rec.mimeType || tipo }));
        rec.stop();
      });
    },
  };
}

/** Trascrive un Blob audio; restituisce il testo (vuoto se non si è sentito niente). */
export async function trascrivi(audio) {
  const est = (audio.type || '').includes('mp4') ? 'mp4' : (audio.type || '').includes('ogg') ? 'ogg' : 'webm';
  const f = new File([audio], `dettato.${est}`, { type: audio.type || 'audio/webm' });
  const lingua = assistenteDi(stato.assistenteId).voce_lingua || 'it';
  const r = await api.trascrivi(f, lingua);
  return (r?.testo || '').trim();
}

// ---------- Riproduzione con coda ----------
const coda = [];
let inRiproduzione = null; // {audio, url, fine}
const lettore = new Audio();
/** L'elemento audio della sintesi: la chiamata lo collega a un analizzatore per l'onda e l'avatar. */
export const lettoreVoce = () => lettore;
lettore.preload = 'auto';

/** Toglie Markdown e codice: alla voce va il testo leggibile. */
export function testoParlabile(md) {
  return (md || '')
    .replace(/```[\s\S]*?```/g, ' (blocco di codice) ')
    .replace(/`([^`]+)`/g, '$1')
    .replace(/!\[[^\]]*\]\([^)]*\)/g, '')
    .replace(/\[([^\]]+)\]\([^)]*\)/g, '$1')
    .replace(/^#+\s*/gm, '')
    .replace(/[*_~>|]/g, '')
    .replace(/\n{2,}/g, '. ')
    .replace(/\s+/g, ' ')
    .trim()
    .slice(0, 4000);
}

/** Mette in coda la sintesi di un testo con la voce dell'assistente. Risolve a fine lettura. */
export function parla(testo, assistenteId = stato.assistenteId) {
  const pulito = testoParlabile(testo);
  if (!pulito) return Promise.resolve();
  return new Promise((risolvi) => {
    coda.push({ testo: pulito, assistenteId, risolvi });
    if (!inRiproduzione) prossimo();
  });
}

async function prossimo() {
  const v = coda.shift();
  if (!v) { inRiproduzione = null; bus.emetti('voce:parla', false); return; }
  inRiproduzione = v;
  bus.emetti('voce:parla', true);
  const a = assistenteDi(v.assistenteId);
  const velocita = Number(a.voce_velocita || impostazione('interfaccia.voce_velocita', 1)) || 1;
  try {
    const blob = await api.sintesi(v.testo, a.voce_id || impostazione('interfaccia.voce', '') || undefined, velocita);
    if (inRiproduzione !== v) return; // fermata intanto
    const url = URL.createObjectURL(blob);
    lettore.src = url;
    await new Promise((fine) => {
      lettore.onended = fine;
      lettore.onerror = fine;
      v.fine = fine;
      lettore.play().catch((e) => { avvisa(e.name === 'NotAllowedError' ? 'Tocca la pagina per sentire la voce' : 'Riproduzione non riuscita'); fine(); });
    });
    URL.revokeObjectURL(url);
  } catch (e) {
    avvisa(`Voce non disponibile: ${testoErrore(e)}`, 3500);
  }
  v.risolvi();
  if (inRiproduzione === v) prossimo();
}

/** Ferma la voce e svuota la coda. */
export function zitto() {
  for (const v of coda.splice(0)) v.risolvi();
  const v = inRiproduzione;
  inRiproduzione = null;
  lettore.pause();
  v?.fine?.();
  bus.emetti('voce:parla', false);
}

export const staParlando = () => !!inRiproduzione;

// ---------- Risposta parlata a fine turno ----------
export const rispostaParlata = () => impostazione(CHIAVE_PARLATA, false) === true;

const visti = new Set(); // turni visti «in corso» in questa pagina: si legge solo la loro fine
bus.su('flusso:turno', ({ sessioneId, turno_id, stato: st }) => {
  if (st === 'in_corso') { visti.add(turno_id); return; }
  if (st !== 'riuscito' || sessioneId !== stato.sessione?.id || !visti.delete(turno_id)) return;
  if (!rispostaParlata() || document.visibilityState !== 'visible') return;
  if (bus.manilibere) return; // in mani libere ci pensa il suo giro
  setTimeout(() => { const t = bus.testoDelTurno?.(turno_id); if (t) parla(t); }, 300);
});

/** Per la pagina Generali: l'elenco dei microfoni (le etichette arrivano dopo il permesso). */
export async function microfoni() {
  try { return (await navigator.mediaDevices.enumerateDevices()).filter((d) => d.kind === 'audioinput'); } catch { return []; }
}
