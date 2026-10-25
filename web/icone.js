// Icone SVG in linea, a tratto: 24×24, colore dal testo.

const tratti = {
  cerca: '<circle cx="11" cy="11" r="6.5"/><path d="m20 20-4.2-4.2"/>',
  piu: '<path d="M12 5v14M5 12h14"/>',
  chiudi: '<path d="M6 6l12 12M18 6 6 18"/>',
  giu: '<path d="m6 9 6 6 6-6"/>',
  destra: '<path d="m9 6 6 6-6 6"/>',
  menu: '<path d="M4 7h16M4 12h16M4 17h16"/>',
  info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7.5v.5"/>',
  microfono: '<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21"/>',
  onda: '<path d="M4 10v4M8 7v10M12 4v16M16 7v10M20 10v4"/>',
  invia: '<path d="M12 19V5M6 11l6-6 6 6"/>',
  presa: '<path d="M9 3v5M15 3v5M6 8h12v3a6 6 0 0 1-12 0zM12 17v4"/>',
  impostazioni: '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.6 1.6 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.6 1.6 0 0 0-1.8-.3 1.6 1.6 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.6 1.6 0 0 0-1-1.5 1.6 1.6 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.6 1.6 0 0 0 .3-1.8 1.6 1.6 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.6 1.6 0 0 0 1.5-1 1.6 1.6 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.6 1.6 0 0 0 1.8.3H9a1.6 1.6 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.6 1.6 0 0 0 1 1.5 1.6 1.6 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.6 1.6 0 0 0-.3 1.8V9a1.6 1.6 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.6 1.6 0 0 0-1.5 1z"/>',
  computer: '<rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/>',
  scudo: '<path d="M12 3 5 6v5c0 4.5 3 8.3 7 10 4-1.7 7-5.5 7-10V6z"/>',
  chiave: '<circle cx="8" cy="15" r="4"/><path d="m11 12 9-9M17 6l3 3M15 8l2 2"/>',
  anello: '<circle cx="12" cy="12" r="8.5"/><path d="M12 3.5A8.5 8.5 0 0 1 20.5 12"/>',
  canali: '<path d="M4 12h4l3-7 3 14 3-7h3"/>',
  grafico: '<path d="M4 20V10M10 20V4M16 20v-7M22 20H2"/>',
  aggiorna: '<path d="M12 4v10M7 9l5 5 5-5M5 20h14"/>',
  esci: '<path d="M15 4h4v16h-4M10 8l-4 4 4 4M6 12h10"/>',
  luna: '<path d="M20 14.5A8 8 0 0 1 9.5 4 8 8 0 1 0 20 14.5z"/>',
  sessioni: '<path d="M4 6h16M4 12h10M4 18h7"/>',
  strumento: '<path d="M14.5 6.5a4 4 0 0 0-5.4 5l-5.6 5.6 2.4 2.4 5.6-5.6a4 4 0 0 0 5-5.4l-2.3 2.3-2-2z"/>',
  avviso: '<path d="M12 4 2.5 20h19zM12 10v4.5M12 17.3v.2"/>',
  spunta: '<path d="m5 12.5 4.5 4.5L19 7.5"/>',
  copia: '<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a1 1 0 0 1 1-1h9"/>',
  rispondi: '<path d="M9 7 4 12l5 5M4 12h11a5 5 0 0 1 5 5v1"/>',
  archivia: '<rect x="3" y="4" width="18" height="5" rx="1"/><path d="M5 9v10h14V9M10 13h4"/>',
  matita: '<path d="m4 20 4-1 11-11-3-3L5 16zM14 6l3 3"/>',
  stella: '<path d="m12 4 2.4 5 5.6.6-4.2 3.8 1.2 5.6-5-2.9-5 2.9 1.2-5.6L4 9.6 9.6 9z"/>',
  ferma: '<rect x="7" y="7" width="10" height="10" rx="2"/>',
  altoparlante: '<path d="M4 9h4l5-4v14l-5-4H4zM16.5 8.5a5 5 0 0 1 0 7M19 6a8.5 8.5 0 0 1 0 12"/>',
  file: '<path d="M6 3h8l4 4v14H6zM14 3v4h4"/>',
  graffetta: '<path d="m20 11-8.5 8.5a5 5 0 0 1-7-7L13 4a3.5 3.5 0 0 1 5 5l-8.5 8.5a2 2 0 0 1-3-3L14 7"/>',
  occhio: '<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/>',
  occhiochiuso: '<path d="M3 3l18 18M10.6 5.1A10 10 0 0 1 12 5c6.5 0 10 7 10 7a17 17 0 0 1-3.2 4.2M6.6 6.6C3.8 8.4 2 12 2 12s3.5 7 10 7a9.6 9.6 0 0 0 5.4-1.6M9.9 9.9a3 3 0 0 0 4.2 4.2"/>',
  cestino: '<path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/>',
  play: '<path d="M7 4.5v15l12-7.5z"/>',
  immagine: '<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="9" cy="10" r="2"/><path d="m21 16-5-5-9 9"/>',
  campana: '<path d="M6 16V11a6 6 0 0 1 12 0v5l2 2H4zM10 21h4"/>',
  scarica: '<path d="M12 4v11M7 10l5 5 5-5M5 20h14"/>',
  carica: '<path d="M12 20V9M7 14l5-5 5 5M5 4h14"/>',
  carta: '<rect x="2.5" y="5" width="19" height="14" rx="2"/><path d="M2.5 10h19M6 15h4"/>',
  persona: '<circle cx="12" cy="8" r="4"/><path d="M4 21a8 8 0 0 1 16 0"/>',
  telefono: '<rect x="7" y="2.5" width="10" height="19" rx="2"/><path d="M11 18.5h2"/>',
  fumetto: '<path d="M4 5h16v11H9l-5 4z"/>',
  micmuto: '<path d="M3 3l18 18M9 9v2a3 3 0 0 0 5 2.2M15 10V6a3 3 0 0 0-5.8-1M5.5 11a6.5 6.5 0 0 0 10.6 5M18.5 11a6.5 6.5 0 0 1-.6 2.7M12 17.5V21"/>',
  pollicesu: '<path d="M7 11v9H4v-9zM7 11l4-7a2 2 0 0 1 2 2v4h5a2 2 0 0 1 2 2.3l-1.2 6A2 2 0 0 1 16.8 20H7"/>',
  pollicegiu: '<path d="M7 13V4H4v9zM7 13l4 7a2 2 0 0 0 2-2v-4h5a2 2 0 0 0 2-2.3l-1.2-6A2 2 0 0 0 16.8 4H7"/>',
  orologio: '<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/>',
  tastiera: '<rect x="2.5" y="6" width="19" height="12" rx="2"/><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M6 14h.01M18 14h.01M9 14h6"/>',
  meno: '<path d="M5 12h14"/>',
  adatta: '<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>',
  appunti: '<rect x="6" y="4" width="12" height="17" rx="2"/><path d="M9 4V3h6v1M9 10h6M9 14h6"/>',
  altro: '<circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/>',
  lucchetto: '<rect x="5" y="10" width="14" height="11" rx="2"/><path d="M8 10V7a4 4 0 0 1 8 0v3"/>',
};

/** Un'icona come elemento SVG. */
export function icona(nome, misura = 18, classe = '') {
  const ns = 'http://www.w3.org/2000/svg';
  const s = document.createElementNS(ns, 'svg');
  s.setAttribute('viewBox', '0 0 24 24');
  s.setAttribute('width', misura);
  s.setAttribute('height', misura);
  s.setAttribute('fill', 'none');
  s.setAttribute('stroke', 'currentColor');
  s.setAttribute('stroke-width', '1.8');
  s.setAttribute('stroke-linecap', 'round');
  s.setAttribute('stroke-linejoin', 'round');
  s.setAttribute('aria-hidden', 'true');
  if (classe) s.setAttribute('class', classe);
  s.innerHTML = tratti[nome] || '';
  return s;
}

/** Mette l'icona dentro un tasto che ha solo l'icona. */
export function iconaIn(tasto, nome, misura = 18) {
  if (tasto) tasto.replaceChildren(icona(nome, misura));
}

/** L'anello della quota: percentuale USATA 0–100 (◔ = poco usata); rosso quando resta meno del 10 %. */
export function anello(usata, misura = 14, spessore = 2.4) {
  const ns = 'http://www.w3.org/2000/svg';
  const r = (misura - spessore) / 2;
  const giro = 2 * Math.PI * r;
  const u = usata === null || usata === undefined ? null : Math.max(0, Math.min(100, usata));
  const s = document.createElementNS(ns, 'svg');
  s.setAttribute('viewBox', `0 0 ${misura} ${misura}`);
  s.setAttribute('width', misura);
  s.setAttribute('height', misura);
  s.setAttribute('aria-hidden', 'true');
  const c = misura / 2;
  s.innerHTML = `<circle class="anello-traccia" cx="${c}" cy="${c}" r="${r}" fill="none" stroke-width="${spessore}"/>`
    + (u === null ? '' : `<circle class="anello-valore${u > 90 ? ' bassa' : ''}" cx="${c}" cy="${c}" r="${r}" fill="none" stroke-width="${spessore}"
        stroke-linecap="round" stroke-dasharray="${giro}" stroke-dashoffset="${giro * (1 - u / 100)}" transform="rotate(-90 ${c} ${c})"/>`);
  return s;
}
