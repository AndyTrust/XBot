// Smistamento di ciò che l'utente dice a voce mentre una chat vocale è già in corso (richiesta 2026-10-25):
// regole esplicite, nessun modello. Restituisce
//   {tipo: 'ferma'}                                  «basta», «aspetta»: zitti e si interrompe il turno
//   {tipo: 'parallelo', assistente|null, testo}      «chiedi a Sara …», «intanto …», «un'altra cosa …»
//   {tipo: 'continua', testo}                        tutto il resto: stessa sessione, stesso assistente
// `assistente` null = nessun nome detto: sessione nuova dello stesso assistente.

const senza = (s) => (s || '').normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();
const parole = (s) => senza(s).replace(/[^a-z0-9@_\s.-]/g, ' ').split(/\s+/).filter(Boolean);

const FERMA = /^(basta|stop|fermati|ferma|zitto|zitta|silenzio|aspetta|aspetti|annulla|lascia stare)( (cosi|un attimo|un momento|un secondo|per favore|grazie))?[.!\s]*$/;
// «chiedi a», «dillo a», «passa a», «manda a», «fallo fare a», «affida a» …
const VERBO_NOME = /\b(?:chiedi|chiedilo|chiedere|dillo|di|passa|passalo|manda|mandalo|affida|affidalo|fai fare|fallo fare|lascia|girala|giralo)\s+(?:questo\s+|tutto\s+)?(?:a|ad)\s+(@?[a-z0-9_.-]+)/;
const STACCO = /\b(?:intanto|nel frattempo|un'altra cosa|un altra cosa|altra cosa|altra richiesta|nuova richiesta|cambiando argomento|in parallelo)\b/;

/** L'assistente il cui nome (o primo nome) o menzione è `parola`. */
function trovaAssistente(parola, assistenti) {
  const p = parola.replace(/^@/, '').replace(/[.]+$/, '');
  if (p.length < 2) return null;
  return assistenti.find((a) => !a.archiviato_ms && (senza(a.menzione) === p || senza(a.nome) === p || senza(a.nome).split(/\s+/)[0] === p)) || null;
}

/** Toglie dal testo originale la parte di comando («chiedi a Sara», «intanto») e lascia la richiesta. */
function richiesta(originale, marcatori) {
  let t = originale.trim();
  for (const m of marcatori) {
    const n = senza(t);
    const r = m.exec(n);
    if (r) t = (t.slice(0, r.index) + ' ' + t.slice(r.index + r[0].length)).trim();
  }
  t = t.replace(/^[\s,.:;!-]+/, '').replace(/^(?:di|per favore|puoi|può)\s+/i, '').replace(/^[\s,.:;!-]+/, '').trim();
  return t || originale.trim();
}

export function classifica(testo, assistenti, correnteId) {
  const originale = (testo || '').trim();
  const norm = senza(originale).replace(/[?!]+$/, '');
  if (!norm) return { tipo: 'continua', testo: originale };
  if (FERMA.test(norm)) return { tipo: 'ferma' };
  const lista = [...assistenti];

  const m = VERBO_NOME.exec(norm);
  let altro = m ? trovaAssistente(m[1], lista) : null;
  let marcatori = altro ? [VERBO_NOME, /\bintanto\b|\bnel frattempo\b/] : [];
  if (!altro) {
    // «@sara» arrivato dal dettato
    const at = parole(norm).find((p) => p.startsWith('@') && trovaAssistente(p, lista));
    if (at) { altro = trovaAssistente(at, lista); marcatori = [new RegExp(`@${at.slice(1).replace(/[.]+$/, '')}`)]; }
  }
  if (altro && altro.id === correnteId) return { tipo: 'continua', testo: richiesta(originale, marcatori) };
  if (altro) return { tipo: 'parallelo', assistente: altro, testo: richiesta(originale, marcatori) };
  if (STACCO.test(norm)) return { tipo: 'parallelo', assistente: null, testo: richiesta(originale, [STACCO]) };
  return { tipo: 'continua', testo: originale };
}

/** Vero se `detto` è quasi tutto parole che l'assistente ha appena pronunciato (eco del microfono). */
export function eEco(detto, pronunciato) {
  const d = parole(detto).filter((p) => p.length > 2);
  if (d.length < 3 || !pronunciato) return false;
  const insieme = new Set(parole(pronunciato));
  return d.filter((p) => insieme.has(p)).length / d.length >= 0.8;
}
