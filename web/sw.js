// Service worker della app: cache dei file statici (prima la rete), notifiche push, badge.
// L'API (/api/) non passa mai dalla cache: va sempre al server.

// La chiave della cache segue la versione installata (`?v=` messo da aggiorna.js da GET /salute):
// a ogni aggiornamento il worker è nuovo, la cache è nuova e `activate` toglie le vecchie.
const V = new URL(self.location.href).searchParams.get('v');
const VERSIONE = V ? `xbot-web-${V.replace(/[^\w.-]+/g, '_').slice(0, 60)}` : 'xbot-web-2e-1';
const NOME_APP = "XBot"; // riscritto da prove/rinomina.py insieme a nome.js
const FILE = ['/app/', '/app/index.html', '/app/stile.css', '/app/finestre.css', '/app/componenti.css', '/app/pagine.css',
  '/app/manifest.webmanifest', '/app/app.js', '/app/icone/app.svg', '/app/icone/app-192.png'];

self.addEventListener('install', (ev) => {
  ev.waitUntil(caches.open(VERSIONE).then((c) => c.addAll(FILE)).catch(() => {}).then(() => self.skipWaiting()));
});

self.addEventListener('activate', (ev) => {
  ev.waitUntil(caches.keys()
    .then((chiavi) => Promise.all(chiavi.filter((k) => k !== VERSIONE).map((k) => caches.delete(k))))
    .then(() => self.clients.claim()));
});

// Prima la rete (così un aggiornamento arriva subito), la cache solo se la rete manca.
self.addEventListener('fetch', (ev) => {
  const url = new URL(ev.request.url);
  if (ev.request.method !== 'GET' || url.origin !== location.origin || url.pathname.startsWith('/api/') || url.pathname === '/mcp') return;
  ev.respondWith(fetch(ev.request).then((r) => {
    if (r.ok) { const copia = r.clone(); caches.open(VERSIONE).then((c) => c.put(ev.request, copia)); }
    return r;
  }).catch(() => caches.match(ev.request).then((r) => r || caches.match('/app/index.html'))));
});

// Notifica dal server: {titolo, corpo, assistente_id?, sessione_id?, in_attesa?}
self.addEventListener('push', (ev) => {
  let d = {};
  try { d = ev.data ? ev.data.json() : {}; } catch { d = { corpo: ev.data?.text() }; }
  const titolo = (d.titolo || d.title || NOME_APP).slice(0, 200);
  const corpo = (d.corpo || d.body || '').slice(0, 140);
  const dati = { assistente_id: d.assistente_id, sessione_id: d.sessione_id };
  ev.waitUntil(Promise.all([
    self.registration.showNotification(titolo, { body: corpo, icon: '/app/icone/app-192.png', badge: '/app/icone/app-192.png', tag: d.sessione_id || d.assistente_id || undefined, data: dati, renotify: true }),
    Number.isFinite(d.in_attesa) && self.navigator.setAppBadge ? (d.in_attesa > 0 ? self.navigator.setAppBadge(d.in_attesa) : self.navigator.clearAppBadge()) : null,
  ]));
});

// Tocco sulla notifica: apre (o mette in primo piano) la app sulla sessione.
self.addEventListener('notificationclick', (ev) => {
  ev.notification.close();
  const d = ev.notification.data || {};
  const p = new URLSearchParams();
  if (d.assistente_id) p.set('assistente', d.assistente_id);
  if (d.sessione_id) p.set('sessione', d.sessione_id);
  const url = `/app/${p.toString() ? `?${p}` : ''}`;
  ev.waitUntil(self.clients.matchAll({ type: 'window', includeUncontrolled: true }).then((finestre) => {
    for (const f of finestre) {
      if (new URL(f.url).origin === location.origin) {
        f.postMessage({ tipo: 'apri', assistente_id: d.assistente_id, sessione_id: d.sessione_id });
        return f.focus();
      }
    }
    return self.clients.openWindow(url);
  }));
});
