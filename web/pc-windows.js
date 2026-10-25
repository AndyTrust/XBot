// Computer Windows (il PC dell'amministrazione) nella sezione Computer: terminale (tmux «pc» via ttyd)
// e schermo (noVNC), serviti da XBot con il suo accesso. Si apre a pieno schermo sopra tutto.

import { api } from './api.js';
import { icona } from './icone.js';
import { el } from './util.js';

let aperto = null;

/** La voce c'è solo se il server ha `XBOT_PC_WINDOWS`; altrimenti `null`. */
export async function infoPcWindows() {
  try {
    const r = await api.pcWindows();
    return r?.attivo ? r : null;
  } catch { return null; }
}

function chiudi() {
  if (!aperto) return;
  if (document.fullscreenElement === aperto) document.exitFullscreen?.().catch(() => {});
  aperto.remove();
  aperto = null;
}

/** Apre il terminale (o lo schermo) del Computer Windows a pieno schermo. `dove`: 'terminale' | 'schermo'. */
export function apriPcWindows(info, dove = 'terminale') {
  chiudi();
  const cornice = el('iframe.pcw-iframe', { title: 'Computer Windows', allow: 'clipboard-read; clipboard-write; fullscreen' });
  const scheda = (id, testo) => el('button', { type: 'button', role: 'tab', 'aria-selected': String(dove === id), onclick: () => { dove = id; mostra(); } }, testo);
  const schede = el('div.schede-pillola', { role: 'tablist', 'aria-label': 'Terminale o schermo' }, scheda('terminale', 'Terminale'), scheda('schermo', 'Schermo'));
  const mostra = () => {
    cornice.src = dove === 'schermo' ? info.schermo : info.terminale;
    schede.querySelectorAll('button').forEach((b, i) => b.setAttribute('aria-selected', String(['terminale', 'schermo'][i] === dove)));
  };
  aperto = el('div.pcw-pieno', { role: 'dialog', 'aria-label': 'Computer Windows a pieno schermo' },
    el('div.pcw-barra', {}, el('strong', {}, info.nome || 'Computer Windows'), schede, el('span.spazio'),
      el('button.tasto-icona.piccolo', { type: 'button', 'aria-label': 'Chiudi', title: 'Chiudi (Esc)', onclick: chiudi }, icona('chiudi', 14))),
    cornice);
  document.body.append(aperto);
  mostra();
  // il pieno schermo vero del browser, se lo concede; altrimenti resta la copertura della finestra
  aperto.requestFullscreen?.().catch(() => {});
  document.addEventListener('fullscreenchange', () => { if (aperto && !document.fullscreenElement) chiudi(); }, { once: true });
  document.addEventListener('keydown', function esc(ev) { if (ev.key === 'Escape') { chiudi(); document.removeEventListener('keydown', esc); } });
}
