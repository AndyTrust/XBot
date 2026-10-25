// Notifiche push (CONTRATTO-2D sez. 8): dopo l'accesso chiede il permesso con una spiegazione,
// GET /push/chiave, pushManager.subscribe, POST /push/abbonamento. Badge sull'icona con il numero
// di assistenti in attesa di te.

import { api, testoErrore } from './api.js';
import { icona } from './icone.js';
import { NOME_APP } from './nome.js';
import { apriSovrapposta } from './menu.js';
import { registraSw } from './aggiorna.js';
import { avvisa, el, locale } from './util.js';

const RINVIO_MS = 7 * 86400000;

export function statoNotifiche() {
  const possibili = 'Notification' in window && 'serviceWorker' in navigator && 'PushManager' in window && isSecureContext;
  const permesso = possibili ? Notification.permission : 'denied';
  return { possibili, negate: permesso === 'denied' && possibili, attive: permesso === 'granted' && !!locale.leggi('push.abbonato') };
}

function chiaveDa(b64) {
  const s = (b64 + '='.repeat((4 - (b64.length % 4)) % 4)).replace(/-/g, '+').replace(/_/g, '/');
  return Uint8Array.from(atob(s), (c) => c.charCodeAt(0));
}

const b64url = (buf) => btoa(String.fromCharCode(...new Uint8Array(buf))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');

async function registrazione() {
  return (await navigator.serviceWorker.getRegistration('/app/')) || registraSw();
}

/** Si abbona e registra l'abbonamento sul server. */
async function abbona() {
  const r = await api.chiavePush();
  const chiave = typeof r === 'string' ? r : r?.chiave || r?.chiave_pubblica || r?.pubblica;
  if (!chiave) throw new Error('Il server non ha la chiave VAPID');
  const reg = await registrazione();
  let ab = await reg.pushManager.getSubscription();
  if (!ab) ab = await reg.pushManager.subscribe({ userVisibleOnly: true, applicationServerKey: chiaveDa(chiave) });
  await api.abbonaPush({
    endpoint: ab.endpoint,
    chiavi: { p256dh: b64url(ab.getKey('p256dh')), auth: b64url(ab.getKey('auth')) },
    dispositivo: locale.leggi('dispositivo', '') || navigator.userAgentData?.platform || 'browser',
  });
  locale.scrivi('push.abbonato', ab.endpoint);
}

/** Chiede il permesso (con il foglio di spiegazione se `spiega`) e si abbona. */
export function chiediNotifiche({ spiega = true } = {}) {
  const s = statoNotifiche();
  if (!s.possibili) { avvisa('Le notifiche non sono disponibili in questo browser'); return Promise.resolve(false); }
  const procedi = async () => {
    try {
      const p = await Notification.requestPermission();
      if (p !== 'granted') { avvisa(p === 'denied' ? 'Notifiche negate: si riattivano dalle impostazioni del sito' : 'Notifiche non attivate'); return false; }
      await abbona();
      avvisa('Notifiche attive');
      return true;
    } catch (e) { avvisa(`Notifiche non attivate: ${testoErrore(e)}`, 4000); return false; }
  };
  if (!spiega) return procedi();
  return new Promise((risolvi) => {
    let fatto = false;
    const { chiudi } = apriSovrapposta([
      el('div.foglio-testa', {}, icona('campana', 20), el('h2', {}, 'Vuoi le notifiche?')),
      el('div.foglio-corpo.una', {}, el('p', {}, `${NOME_APP} ti avvisa quando un assistente ha finito e tocca a te, quando chiede un permesso e quando qualcosa va storto. Niente altro.`),
        el('p.nota', {}, 'Il browser ti chiederà la conferma. Puoi cambiare idea dalle impostazioni, sezione Generali.')),
      el('div.foglio-piede', {},
        el('button.tasto', { type: 'button', onclick: () => { locale.scrivi('push.rinviato', Date.now()); chiudi(); } }, 'Non ora'),
        el('button.tasto.primario', { type: 'button', onclick: async () => { fatto = true; chiudi(); risolvi(await procedi()); } }, 'Attiva le notifiche')),
    ], { classe: 'foglio.piccolo', etichetta: 'Notifiche', quandoChiusa: () => { if (!fatto) risolvi(false); } });
  });
}

/** Dopo l'accesso: chiede una volta (poi ogni 7 giorni se rinviato); se già concesso, rinnova l'abbonamento. */
export async function dopoAccesso() {
  const s = statoNotifiche();
  if (!s.possibili) return;
  if (Notification.permission === 'granted') { abbona().catch(() => {}); return; }
  if (Notification.permission === 'default' && Date.now() - Number(locale.leggi('push.rinviato', 0)) > RINVIO_MS) {
    setTimeout(() => chiediNotifiche({ spiega: true }), 1500);
  }
}

/** Il numero sull'icona della app. */
export function aggiornaBadge(n) {
  try {
    if (n > 0) navigator.setAppBadge?.(n); else navigator.clearAppBadge?.();
  } catch { /* non supportato */ }
}
