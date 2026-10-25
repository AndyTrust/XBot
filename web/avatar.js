// Avatar degli assistenti, disegnati sempre da qui (barra, pillola, pannello, chat, fogli):
// - una delle 8 forme con due occhi (viso semplice, riconoscibile anche piccolo);
// - un'icona della galleria `web/avatar/<nome>.svg` (monocroma, ricolorata col colore scelto);
// - un personaggio a colori `web/avatar/personaggi/` (avatar_forma = "personaggio:<nome>");
// - una foto caricata (`avatar_url`, o `avatar_percorso` se è già un indirizzo /api/ o un hash).

export const FORME = {
  cerchio: { nome: 'Cerchio', d: 'M20 3a17 17 0 1 1 0 34a17 17 0 1 1 0-34z' },
  quadrato: { nome: 'Quadrato', d: 'M11 4h18a7 7 0 0 1 7 7v18a7 7 0 0 1-7 7H11a7 7 0 0 1-7-7V11a7 7 0 0 1 7-7z' },
  goccia: { nome: 'Goccia', d: 'M20 3c6 7 15 14 15 21a15 13 0 0 1-30 0C5 17 14 10 20 3z' },
  esagono: { nome: 'Esagono', d: 'M20 3l14.5 8.5v17L20 37 5.5 28.5v-17z' },
  macchia: { nome: 'Macchia', d: 'M8 12c3-7 16-9 23-5s7 14 4 20-12 10-19 8S2 27 3 21s2-6 5-9z' },
  fiore: { nome: 'Fiore', d: 'M20 3l4 7 8-2-2 8 7 4-7 4 2 8-8-2-4 7-4-7-8 2 2-8-7-4 7-4-2-8 8 2z' },
  nuvola: { nome: 'Nuvola', d: 'M12 34a9 9 0 0 1-2-17.7A11 11 0 0 1 31 14a9 9 0 0 1-1 18z' },
  rombo: { nome: 'Rombo', d: 'M20 3c2 0 3 1 5 3l9 9c3 3 3 7 0 10l-9 9c-3 3-7 3-10 0l-9-9c-3-3-3-7 0-10l9-9c2-2 3-3 5-3z' },
};

export const COLORI = {
  '#1c1c1e': 'Nero',
  '#34b36b': 'Verde',
  '#ec4f9a': 'Rosa',
  '#ef4444': 'Rosso',
  '#7c5cff': 'Viola',
  '#3b82f6': 'Blu',
  '#f59e0b': 'Arancio',
  '#14b8a6': 'Turchese',
};

/** Le icone della galleria (file in web/avatar/, fill="currentColor"). */
export const ICONE = ['robot', 'gatto', 'orso', 'coniglio', 'gufo', 'volpe', 'pesce', 'uccello', 'tartaruga', 'razzo', 'aereo',
  'stella', 'cuore', 'fulmine', 'foglia', 'fiamma', 'luna', 'sole', 'casa', 'chiave', 'lampadina', 'libro', 'ingranaggio',
  'ancora', 'tazza', 'busta', 'scudo', 'corona', 'diamante', 'globo', 'nota', 'fotocamera', 'bilancia', 'grafico'];

/** I personaggi a colori (indice in web/avatar/personaggi/indice.json): nome → {nome, file, etichetta}. */
export const PERSONAGGI = new Map();
export async function caricaPersonaggi() {
  if (PERSONAGGI.size) return PERSONAGGI;
  try {
    const r = await fetch('/app/avatar/personaggi/indice.json');
    for (const v of await r.json()) PERSONAGGI.set(v.nome, v);
  } catch { /* senza indice restano forme e icone */ }
  return PERSONAGGI;
}

/** Il personaggio che combacia con un nome o una menzione (postino, manutentore, ricercatore-web…). */
export function personaggioPer(nome = '') {
  const n = nome.toLocaleLowerCase('it').trim().replace(/[\s_]+/g, '-');
  if (!n) return null;
  for (const k of PERSONAGGI.keys()) if (k === n || k === `nuovo-${n}`) return k;
  // un nome che è l'inizio di un personaggio con variante («gufo» → «gufo-saggio»)
  for (const k of PERSONAGGI.keys()) if (k.startsWith(`${n}-`)) return k;
  return null;
}

const elencoForme = Object.keys(FORME);
const elencoColori = Object.keys(COLORI);

/** Forma e colore stabili da un nome, quando l'assistente non li ha scelti. */
function daNome(nome = '') {
  let h = 0;
  for (const c of nome) h = (h * 31 + c.codePointAt(0)) >>> 0;
  return { forma: elencoForme[h % elencoForme.length], colore: elencoColori[(h >> 3) % elencoColori.length] };
}

export function coloreDi(a) {
  const c = a?.avatar_colore;
  return c && /^#[0-9a-f]{6}$/i.test(c) ? c : daNome(a?.nome).colore;
}

export function formaDi(a) {
  const f = a?.avatar_forma;
  if (FORME[f] || ICONE.includes(f) || (typeof f === 'string' && f.startsWith('personaggio:'))) return f;
  const p = personaggioPer(a?.nome);
  return p ? `personaggio:${p}` : daNome(a?.nome).forma;
}

/** L'indirizzo della foto, se c'è. */
export function fotoDi(a) {
  if (a?.avatar_url) return a.avatar_url;
  const p = a?.avatar_percorso;
  if (typeof p === 'string' && p.startsWith('/api/')) return p;
  if (typeof p === 'string' && /^[0-9a-f]{32,128}$/i.test(p)) return `/api/v1/blob/${p}`;
  return null;
}

/** Il disegno dell'avatar (senza pallino). `a` = {nome, avatar_forma, avatar_colore, avatar_url}. */
export function avatarSvg(a, misura = 32, { senzaFoto = false } = {}) {
  const foto = senzaFoto ? null : fotoDi(a);
  if (foto) {
    const img = document.createElement('img');
    img.className = 'avatar-foto';
    img.src = foto;
    img.alt = '';
    img.width = misura;
    img.height = misura;
    img.onerror = () => img.replaceWith(avatarSvg(a, misura, { senzaFoto: true }));
    return img;
  }
  const forma = formaDi(a);
  if (forma.startsWith('personaggio:')) {
    const nome = forma.slice(12);
    const img = document.createElement('img');
    img.className = 'avatar-foto personaggio';
    img.alt = '';
    img.width = misura;
    img.height = misura;
    img.decoding = 'async';
    const file = PERSONAGGI.get(nome)?.file;
    img.src = `/app/avatar/personaggi/${file || `${nome}.webp`}`;
    img.onerror = () => {
      if (!file && img.src.endsWith('.webp')) { img.src = `/app/avatar/personaggi/${nome}.png`; return; }
      img.replaceWith(avatarSvg({ ...a, avatar_forma: daNome(a?.nome).forma }, misura, { senzaFoto: true }));
    };
    return img;
  }
  if (ICONE.includes(forma)) {
    const box = document.createElement('span');
    box.className = 'avatar-icona';
    box.setAttribute('aria-hidden', 'true');
    box.style.cssText = `width:${misura}px;height:${misura}px;--colore:${coloreDi(a)};--icona:url("/app/avatar/${forma}.svg")`;
    box.append(document.createElement('span'));
    return box;
  }
  const ns = 'http://www.w3.org/2000/svg';
  const s = document.createElementNS(ns, 'svg');
  s.setAttribute('viewBox', '0 0 40 40');
  s.setAttribute('width', misura);
  s.setAttribute('height', misura);
  s.setAttribute('aria-hidden', 'true');
  const colore = coloreDi(a);
  s.innerHTML = `<path d="${FORME[forma].d}" fill="${colore}"/>`
    + '<ellipse cx="16" cy="18" rx="2.2" ry="3.4" fill="#fff"/>'
    + '<ellipse cx="25" cy="18" rx="2.2" ry="3.4" fill="#fff"/>';
  return s;
}

/** Avatar con il pallino di stato (● al lavoro, ◐ in attesa di te). */
export function avatar(a, misura = 32, { stato = true } = {}) {
  const box = document.createElement('span');
  box.className = 'avatar';
  box.append(avatarSvg(a, misura));
  if (stato && (a?.al_lavoro || a?.in_attesa_utente)) {
    const p = document.createElement('span');
    p.className = a.in_attesa_utente ? 'stato attesa' : 'stato';
    p.title = a.in_attesa_utente ? 'In attesa di te' : 'Al lavoro';
    box.append(p);
  }
  return box;
}

/** Lo stato «vivo» di un assistente per l'avatar animato: riposo, lavoro, tocca (in attesa di te). */
export function statoVivo(a, voce = {}) {
  if (voce.in_attesa_utente || a?.in_attesa_utente) return 'tocca';
  if (voce.al_lavoro || a?.al_lavoro) return 'lavoro';
  return 'riposo';
}

/**
 * Avatar grande e animato (barra, chip della chat, pannello): respira a riposo, pulsa al lavoro,
 * fa un piccolo rimbalzo ogni 10 s quando tocca a te. Con `puntino` (barra) un pallino in alto a
 * destra: arancio «tocca a te», blu «non letti», verde pulsante «al lavoro».
 */
export function avatarVivo(a, misura = 56, { voce = {}, puntino = false, corrente = false } = {}) {
  const vivo = statoVivo(a, voce);
  const box = document.createElement('span');
  box.className = `avatar avatar-vivo ${vivo}`;
  box.style.setProperty('--misura', `${misura}px`);
  box.append(avatarSvg(a, misura));
  if (puntino) {
    const tipo = vivo === 'tocca' ? 'attesa' : (voce.non_letti && !corrente) ? 'nuovi' : vivo === 'lavoro' ? 'lavoro' : '';
    if (tipo) {
      const p = document.createElement('span');
      p.className = `puntino ${tipo}`;
      p.title = { attesa: 'Tocca a te', nuovi: `${voce.non_letti} non letti`, lavoro: 'Al lavoro' }[tipo];
      box.append(p);
    }
  }
  return box;
}
