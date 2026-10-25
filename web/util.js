// Aiuti comuni: costruzione del DOM, ULID, tempi, memoria locale, avvisi.

// append, prepend e replaceChildren saltano null, undefined e false (come el()): così i figli
// facoltativi `cond ? nodo : null` non diventano mai il testo «null» nella pagina.
for (const nome of ['append', 'prepend', 'replaceChildren']) {
  for (const proto of [Element.prototype, DocumentFragment.prototype]) {
    const originale = proto[nome];
    proto[nome] = function (...figli) { return originale.apply(this, figli.filter((f) => f !== null && f !== undefined && f !== false)); };
  }
}

/** Crea un elemento: el('div.classe', {attributi}, figli...). */
export function el(sel, attr = {}, ...figli) {
  const [tag, ...classi] = sel.split('.');
  const n = document.createElement(tag || 'div');
  if (classi.length) n.className = classi.join(' ');
  for (const [k, v] of Object.entries(attr || {})) {
    if (v === undefined || v === null || v === false) continue;
    if (k.startsWith('on') && typeof v === 'function') n.addEventListener(k.slice(2), v);
    else if (k === 'dati') Object.assign(n.dataset, v);
    else if (k === 'html') n.innerHTML = v;
    else if (k === 'testo') n.textContent = v;
    else if (v === true) n.setAttribute(k, '');
    else n.setAttribute(k, String(v));
  }
  for (const f of figli.flat()) {
    if (f === null || f === undefined || f === false) continue;
    n.append(f instanceof Node ? f : document.createTextNode(String(f)));
  }
  return n;
}

export const $ = (s, r = document) => r.querySelector(s);

/** Svuota un nodo e ci mette i figli. */
export function riempi(n, ...figli) {
  n.replaceChildren(...figli.flat().filter((f) => f !== null && f !== undefined && f !== false));
}

// ---------- ULID (Crockford base32, 48 bit di tempo + 80 casuali, monotono) ----------
const B32 = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';
let ultimoMs = -1;
let ultimaCoda = null;

export function ulid(ms = Date.now()) {
  let coda;
  if (ms === ultimoMs && ultimaCoda) {
    coda = ultimaCoda.slice();
    for (let i = coda.length - 1; i >= 0; i--) { // +1 con riporto: ordine stretto nello stesso ms
      if (coda[i] < 31) { coda[i]++; break; }
      coda[i] = 0;
    }
  } else {
    const r = crypto.getRandomValues(new Uint8Array(16));
    coda = Array.from(r, (b) => b & 31);
  }
  ultimoMs = ms;
  ultimaCoda = coda;
  let t = '';
  let x = ms;
  for (let i = 0; i < 10; i++) { t = B32[x % 32] + t; x = Math.floor(x / 32); }
  return t + coda.map((c) => B32[c]).join('');
}

// ---------- Tempi ----------
const giorni = ['domenica', 'lunedì', 'martedì', 'mercoledì', 'giovedì', 'venerdì', 'sabato'];
const ora = (d) => d.toLocaleTimeString('it-IT', { hour: 'numeric', minute: '2-digit' });

/** «Oggi 8:20», «Ieri 13:07», «lunedì 9:00», «12/10 9:00». */
export function quandoBreve(ms) {
  const d = new Date(ms);
  const oggi = new Date(); oggi.setHours(0, 0, 0, 0);
  const g = new Date(d); g.setHours(0, 0, 0, 0);
  const diff = Math.round((oggi - g) / 86400000);
  if (diff === 0) return `Oggi ${ora(d)}`;
  if (diff === 1) return `Ieri ${ora(d)}`;
  if (diff > 1 && diff < 7) return `${giorni[d.getDay()]} ${ora(d)}`;
  return `${d.toLocaleDateString('it-IT', { day: 'numeric', month: 'numeric', year: diff > 300 ? '2-digit' : undefined })} ${ora(d)}`;
}

/** «fra 4 gg», «fra 3 h», «fra 12 min». */
export function fraQuanto(ms) {
  const d = ms - Date.now();
  if (!Number.isFinite(d)) return '';
  if (d <= 0) return 'adesso';
  const min = Math.round(d / 60000);
  if (min < 60) return `fra ${min} min`;
  const h = Math.round(min / 60);
  if (h < 48) return `fra ${h} h`;
  return `fra ${Math.round(h / 24)} gg`;
}

export function numero(n, cifre = 0) {
  return Number(n || 0).toLocaleString('it-IT', { maximumFractionDigits: cifre, minimumFractionDigits: cifre });
}

/** 1 234 567 → «1,2 M». */
export function compatto(n) {
  const v = Number(n || 0);
  if (v >= 1e6) return `${numero(v / 1e6, 1)} M`;
  if (v >= 1e4) return `${numero(v / 1e3, 0)} k`;
  return numero(v);
}

// ---------- Memoria locale (può mancare: navigazione privata) ----------
export const locale = {
  leggi(k, predefinito = null) {
    try { const v = localStorage.getItem(`xbot.${k}`); return v === null ? predefinito : JSON.parse(v); } catch { return predefinito; }
  },
  scrivi(k, v) {
    try {
      if (v === null || v === undefined || v === '') localStorage.removeItem(`xbot.${k}`);
      else localStorage.setItem(`xbot.${k}`, JSON.stringify(v));
    } catch { /* memoria piena o negata: si va avanti */ }
  },
};

// ---------- Avviso volante ----------
let timerAvviso;
export function avvisa(testo, ms = 2600) {
  const n = document.getElementById('avviso-volante');
  if (!n) return;
  n.textContent = testo;
  n.hidden = false;
  clearTimeout(timerAvviso);
  timerAvviso = setTimeout(() => { n.hidden = true; }, ms);
}

/** Un piccolo bus di eventi per i moduli. */
export function creaBus() {
  const ascoltatori = new Map();
  return {
    su(nome, f) { (ascoltatori.get(nome) || ascoltatori.set(nome, new Set()).get(nome)).add(f); return () => ascoltatori.get(nome).delete(f); },
    emetti(nome, dati) { for (const f of ascoltatori.get(nome) || []) { try { f(dati); } catch (e) { console.error(e); } } },
  };
}

/** Testo semplice da un valore JSON qualunque, per righe strumento ed esiti. */
export function testoDa(v) {
  if (v === null || v === undefined) return '';
  if (typeof v === 'string') return v;
  try { return JSON.stringify(v, null, 2); } catch { return String(v); }
}

/** Copia negli appunti con l'avviso; restituisce true se riuscita. */
export async function copia(testo, cosa = 'Copiato', nodo = null) {
  try { await navigator.clipboard.writeText(testo); avvisa(cosa); return true; } catch { /* appunti non disponibili: ripiego */ }
  const area = document.createElement('textarea');
  area.value = testo;
  area.setAttribute('readonly', '');
  area.style.cssText = 'position:fixed;top:0;left:0;opacity:0;pointer-events:none';
  document.body.append(area);
  area.select();
  let fatto = false;
  try { fatto = document.execCommand('copy'); } catch { fatto = false; }
  area.remove();
  if (fatto) { avvisa(cosa); return true; }
  if (nodo) { // il testo resta selezionato: basta ⌘C / tieni premuto → Copia
    const r = document.createRange();
    r.selectNodeContents(nodo);
    const sel = getSelection();
    sel.removeAllRanges();
    sel.addRange(r);
  }
  avvisa(nodo ? 'Testo selezionato: copialo con ⌘C (o tieni premuto → Copia)' : 'Copia non riuscita: seleziona e copia a mano', 4000);
  return false;
}

/** 2300 → «2,3 s», 61000 → «1 min 1 s». */
export function durata(ms) {
  if (ms === null || ms === undefined) return '—';
  if (ms < 1000) return `${ms} ms`;
  if (ms < 60000) return `${numero(ms / 1000, 1)} s`;
  return `${Math.floor(ms / 60000)} min ${Math.round((ms % 60000) / 1000)} s`;
}

/** 1536 → «1,5 kB». */
export function dimensione(b) {
  const v = Number(b || 0);
  if (v >= 1048576) return `${numero(v / 1048576, 1)} MB`;
  if (v >= 1024) return `${numero(v / 1024, 1)} kB`;
  return `${v} B`;
}

// ---------- Tasto Indietro (Android, WebAPK): chiude il livello in cima invece dell'app ----------
const livelli = []; // [{chiudi}] dal più vecchio al più nuovo
let daIgnorare = 0;
addEventListener('popstate', () => {
  if (daIgnorare > 0) { daIgnorare--; return; }
  // niente pushState qui: senza un tocco Chrome marca la voce «da saltare» e l'Indietro dopo chiude l'app
  livelli.pop()?.chiudi();
});

/**
 * Mette un livello (tutto schermo, foglio, cassetto) nella cronologia: Indietro chiama `chiudi()`.
 * Restituisce `togli()` per l'uscita volontaria: toglie la voce dalla cronologia una volta sola.
 */
export function livelloIndietro(chiudi) {
  const l = { chiudi };
  history.pushState({ xbotLivello: true }, '');
  livelli.push(l);
  return () => {
    const i = livelli.indexOf(l);
    if (i < 0) return; // già tolto da Indietro
    livelli.splice(i, 1);
    if (i === livelli.length) { daIgnorare++; history.back(); }
  };
}
