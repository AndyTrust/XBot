// «In parallelo …» scritto: apre una chat figlia con il resto del testo come compito.
//   «in parallelo controlla i log»            → figlia dello stesso assistente
//   «@sara in parallelo controlla il calendario» → figlia affidata a @sara
// Solo con la frase esplicita: «intanto» e simili si lasciano al discorso normale. A voce lo smistamento è di
// `smistatore.js` (classifica → tipo `parallelo`), che apre la figlia da `manilibere.js`.

const senza = (s) => (s || '').normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();
const FRASI = '(?:in parallelo|in una chat a parte|in una chat figlia|in una chat annidata)';
const SCHEMA = new RegExp(`^\\s*(@[a-z0-9_.-]+)?\\s*${FRASI}\\s*[,:;-]?\\s+(.+)$`, 's');

/** `{compito, menzione, titolo}` se il testo comincia con la frase esplicita, altrimenti `null`. */
export function riconosciParallelo(testo) {
  const originale = String(testo || '').trim();
  const norm = senza(originale);
  const m = SCHEMA.exec(norm);
  if (!m) return null;
  // la lunghezza non cambia togliendo gli accenti (NFD tolto i segni ma compone di nuovo): si taglia dal testo vero
  const cosi = new RegExp(`^\\s*(?:@[^\\s]+\\s+)?${FRASI.replace(/ /g, '\\s+')}\\s*[,:;-]?\\s+`, 'i').exec(originale.normalize('NFC'));
  const compito = (cosi ? originale.normalize('NFC').slice(cosi[0].length) : m[2]).trim();
  if (compito.length < 3) return null;
  return { compito, menzione: m[1] || '', titolo: compito.slice(0, 60) };
}
