// Un Markdown essenziale, senza dipendenze: titoli, paragrafi, grassetto, corsivo, elenchi,
// citazioni, righe, codice in linea e a blocchi (con «Copia»), link http(s) e mailto.
// Tutto il testo passa per l'escape prima di diventare HTML.

const esc = (s) => s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));

function linkSicuro(url) {
  const u = url.trim();
  return /^(https?:\/\/|mailto:)/i.test(u) ? u : null;
}

/** Formattazione in linea su testo grezzo (una riga o un paragrafo). */
export function inLinea(grezzo) {
  const pezzi = [];
  const segna = (html) => `\u0000${pezzi.push(html) - 1}\u0000`;
  let s = grezzo;
  // codice in linea: prima di tutto, il suo contenuto non si tocca
  s = s.replace(/`([^`\n]+)`/g, (_, c) => segna(`<code>${esc(c)}</code>`));
  // link [testo](url)
  s = s.replace(/\[([^\]\n]+)\]\(([^)\s]+)\)/g, (m, t, u) => {
    const ok = linkSicuro(u);
    return ok ? segna(`<a href="${esc(ok)}" target="_blank" rel="noopener noreferrer">${inLinea(t)}</a>`) : m;
  });
  // link nudi
  s = s.replace(/\bhttps?:\/\/[^\s<>()]+[^\s<>().,;:!?'"»]/g, (u) => segna(`<a href="${esc(u)}" target="_blank" rel="noopener noreferrer">${esc(u)}</a>`));
  s = esc(s);
  s = s.replace(/\*\*(?=\S)([\s\S]*?\S)\*\*/g, '<strong>$1</strong>');
  s = s.replace(/__(?=\S)([\s\S]*?\S)__/g, '<strong>$1</strong>');
  s = s.replace(/(^|[^*\w])\*(?=\S)([^*\n]*?\S)\*(?!\*)/g, '$1<em>$2</em>');
  s = s.replace(/(^|[^_\w])_(?=\S)([^_\n]*?\S)_(?!\w)/g, '$1<em>$2</em>');
  s = s.replace(/~~(?=\S)([^~\n]*?\S)~~/g, '<del>$1</del>');
  // i segnaposto possono annidarsi (link con codice): si risolvono finché ce ne sono
  for (let i = 0; i < 4 && s.includes('\u0000'); i++) {
    s = s.replace(/\u0000(\d+)\u0000/g, (_, n) => pezzi[Number(n)]);
  }
  return s;
}

const reElenco = /^(\s*)([-*+]|\d{1,3}[.)])\s+(.*)$/;

/** Da Markdown a HTML. `inArrivo` lascia aperto un blocco di codice non chiuso. */
export function markdown(testo) {
  const righe = String(testo ?? '').replace(/\r\n?/g, '\n').split('\n');
  const out = [];
  let i = 0;
  let paragrafo = [];
  const chiudiParagrafo = () => {
    if (paragrafo.length) out.push(`<p>${paragrafo.map(inLinea).join('<br>')}</p>`);
    paragrafo = [];
  };
  while (i < righe.length) {
    const r = righe[i];
    const recinto = r.match(/^\s*(```+|~~~+)\s*([\w+#.-]*)\s*$/);
    if (recinto) {
      chiudiParagrafo();
      const fine = recinto[1];
      const lingua = recinto[2];
      const corpo = [];
      i++;
      while (i < righe.length && !righe[i].trim().startsWith(fine)) corpo.push(righe[i++]);
      i++; // la riga di chiusura (o la fine del testo)
      out.push(`<div class="codice"><div class="codice-testa"><span>${esc(lingua || 'codice')}</span>`
        + '<button type="button" data-copia>Copia</button></div>'
        + `<pre><code>${esc(corpo.join('\n'))}</code></pre></div>`);
      continue;
    }
    if (!r.trim()) { chiudiParagrafo(); i++; continue; }
    const titolo = r.match(/^(#{1,6})\s+(.*?)\s*#*\s*$/);
    if (titolo) {
      chiudiParagrafo();
      const n = Math.min(titolo[1].length + 1, 6); // h1 del testo = h2 nella bolla
      out.push(`<h${n}>${inLinea(titolo[2])}</h${n}>`);
      i++;
      continue;
    }
    if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(r)) { chiudiParagrafo(); out.push('<hr>'); i++; continue; }
    if (/^\s*>/.test(r)) {
      chiudiParagrafo();
      const dentro = [];
      while (i < righe.length && /^\s*>/.test(righe[i])) dentro.push(righe[i++].replace(/^\s*>\s?/, ''));
      out.push(`<blockquote>${markdown(dentro.join('\n'))}</blockquote>`);
      continue;
    }
    if (reElenco.test(r)) {
      chiudiParagrafo();
      const [html, dopo] = elenco(righe, i);
      out.push(html);
      i = dopo;
      continue;
    }
    paragrafo.push(r);
    i++;
  }
  chiudiParagrafo();
  return out.join('');
}

/** Un elenco (anche annidato per rientro) a partire dalla riga `i`. */
function elenco(righe, i) {
  const primo = righe[i].match(reElenco);
  const rientro = primo[1].length;
  const ordinato = /\d/.test(primo[2]);
  const inizio = ordinato ? parseInt(primo[2], 10) : 1;
  const voci = [];
  while (i < righe.length) {
    const m = righe[i].match(reElenco);
    if (m && m[1].length === rientro && /\d/.test(m[2]) === ordinato) {
      voci.push({ testo: [m[3]], sotto: [] });
      i++;
    } else if (m && m[1].length > rientro && voci.length) {
      const [html, dopo] = elenco(righe, i);
      voci[voci.length - 1].sotto.push(html);
      i = dopo;
    } else if (!m && righe[i].trim() && /^\s+/.test(righe[i]) && voci.length) {
      voci[voci.length - 1].testo.push(righe[i].trim()); // continuazione della voce
      i++;
    } else {
      break;
    }
  }
  const tag = ordinato ? 'ol' : 'ul';
  const attr = ordinato && inizio !== 1 ? ` start="${inizio}"` : '';
  const html = `<${tag}${attr}>${voci.map((v) => `<li>${v.testo.map(inLinea).join('<br>')}${v.sotto.join('')}</li>`).join('')}</${tag}>`;
  return [html, i];
}

/** Collega i tasti «Copia» dei blocchi di codice dentro `radice`. */
export function attivaCopia(radice, avvisa) {
  radice.addEventListener('click', async (ev) => {
    const b = ev.target.closest('[data-copia]');
    if (!b) return;
    const codice = b.closest('.codice')?.querySelector('code')?.textContent ?? '';
    try {
      await navigator.clipboard.writeText(codice);
      b.textContent = 'Copiato';
      setTimeout(() => { b.textContent = 'Copia'; }, 1400);
    } catch {
      avvisa?.('Copia non riuscita');
    }
  });
}
