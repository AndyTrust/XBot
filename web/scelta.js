// Il foglio di scelta del modello (Fase 2.F): un bottone con la scelta attuale («Mac · Cursor ·
// Grok 4.7 Low») che apre un foglio con la ricerca in testa, i preferiti (stella) e i recenti in
// cima, poi le voci raggruppate per macchina e provider; dentro i provider lunghi (Cursor) le
// famiglie Grok, GPT, Claude, Gemini, Composer, altri. Le varianti «fast» hanno l'etichetta «fast».
// Nessun filtro: ogni modello dichiarato dal server compare. Preferiti e recenti stanno nelle
// impostazioni del server (`PUT /impostazioni`), con una copia nel browser se il server non risponde.
// Lo stesso foglio serve la barra di scrittura, il pannello Dettagli e il foglio dell'assistente.

import { etichettaMacchina, gruppiScelta, inLinea } from './macchine.js';
import { icona } from './icone.js';
import { apriSovrapposta } from './menu.js';
import { impostazione, scriviImpostazione } from './stato.js';
import { el, locale } from './util.js';

export const CHIAVE_PREFERITI = 'interfaccia.modelli_preferiti';
export const CHIAVE_RECENTI = 'interfaccia.modelli_recenti';
const MAX_RECENTI = 6;
const SOGLIA_FAMIGLIE = 16; // sopra queste voci un provider si divide in famiglie

const NOMI_LIVELLI = { none: 'nessuno', minimal: 'minimo', low: 'basso', medium: 'medio', high: 'alto', xhigh: 'altissimo', max: 'massimo', ultra: 'estremo' };
/** «basso (low)»: il nome italiano con il valore tecnico fra parentesi. */
export const etichettaLivello = (l) => (NOMI_LIVELLI[l] ? `${NOMI_LIVELLI[l]} (${l})` : String(l));

const FAMIGLIE = [
  ['grok', 'Grok', /grok/i],
  ['gpt', 'GPT', /\b(gpt|codex|o\d)\b|^gpt|^o\d/i],
  ['claude', 'Claude', /claude|opus|sonnet|haiku/i],
  ['gemini', 'Gemini', /gemini/i],
  ['composer', 'Composer', /composer/i],
];
/** La famiglia di un modello di Cursor: dall'id e dall'etichetta. */
export function famiglia(v) {
  const t = `${v.modello?.id || ''} ${v.etichetta || ''}`;
  for (const [id, nome, re] of FAMIGLIE) if (re.test(t)) return { id, nome };
  return { id: 'altri', nome: 'Altri' };
}

/** «fast»: `dettaglio` «fast» (S, 2.F §14), l'id finisce in `-fast` o l'etichetta in « Fast». */
export const eFast = (v) => /^fast$/i.test(v.modello?.dettaglio || '') || /-fast$/i.test(v.modello?.id || '') || / fast$/i.test(v.etichetta || '');
const altroDettaglio = (v) => { const d = String(v.modello?.dettaglio || '').trim(); return d && !/^fast$/i.test(d) ? d : null; };
const nomeSenzaFast = (v) => (eFast(v) ? String(v.etichetta).replace(/\s+fast$/i, '') : v.etichetta);

function leggiLista(chiave) {
  const v = impostazione(chiave, null);
  if (Array.isArray(v)) return v;
  const l = locale.leggi(chiave, []);
  return Array.isArray(l) ? l : [];
}
async function scriviLista(chiave, v) {
  locale.scrivi(chiave, v);
  try { await scriviImpostazione(chiave, v); } catch { /* resta la copia del browser */ }
}
export const preferiti = () => leggiLista(CHIAVE_PREFERITI);
export const recenti = () => leggiLista(CHIAVE_RECENTI);
export async function cambiaPreferito(valore) {
  const p = preferiti();
  const nuovi = p.includes(valore) ? p.filter((x) => x !== valore) : [...p, valore];
  await scriviLista(CHIAVE_PREFERITI, nuovi);
  return nuovi.includes(valore);
}
async function segnaRecente(valore) {
  await scriviLista(CHIAVE_RECENTI, [valore, ...recenti().filter((x) => x !== valore)].slice(0, MAX_RECENTI));
}

// I modelli principali (2026-10-25): il foglio mostra solo questi; gli altri stanno sotto
// «Altri modelli» e la ricerca li trova sempre. Dentro Cursor il livello di ragionamento fa parte
// del nome del modello: si tiene una sola voce per famiglia, al livello usuale.
const PRINCIPALI = {
  grok: (v) => /^grok-4\.(7|6)$/.test(v.modello?.id || ''),
  cursor_agent: (v) => /^(Auto\b.*|Claude Opus 5\.5 1M|Claude Sonnet 5\.5 Medium|GPT-5\.6 Sol 1M|Gemini 3\.8 Flash Medium|Composer 2\.5|Grok 4\.7 Medium \(Cursor\)|Grok 4\.6 \(Cursor\))$/.test(v.etichetta || '') || v.modello?.id === 'auto',
};
const MAX_SENZA_ELENCO = 6; // un provider con poche voci le mostra tutte
/** Le voci principali di un gruppo (macchina + provider): quelle fisse, la scelta attuale, i preferiti. */
export function principali(gruppo, valoreAttuale = '') {
  const modelli = gruppo.filter((v) => v.modello);
  const regola = PRINCIPALI[gruppo[0].provider?.id];
  if (!regola && modelli.length <= MAX_SENZA_ELENCO) return gruppo;
  let scelte = regola ? modelli.filter(regola) : [];
  if (regola && scelte.length === modelli.length) return gruppo;
  if (!scelte.length) scelte = modelli.slice(0, MAX_SENZA_ELENCO); // elenco cambiato: le prime
  const fissi = new Set(preferiti());
  return gruppo.filter((v) => !v.modello || scelte.includes(v) || v.valore === valoreAttuale || fissi.has(v.valore));
}

const testoProvider = (v) => (v.provider ? (v.provider.nome || v.provider.id) : 'Auto');
const chiaveGruppo = (v) => `${v.macchina?.id || ''}|${v.provider?.id || ''}`;
const testoRicerca = (v) => `${etichettaMacchina(v.macchina)} ${testoProvider(v)} ${v.etichetta || ''} ${v.modello?.id || ''}`.toLowerCase();

/** Quanti modelli ha un provider di una macchina (la voce «predefinito» non conta). */
export const contaModelli = (voci) => voci.filter((v) => v.modello).length;

/**
 * Apre il foglio. `valore` è la scelta attuale («macchina|provider|modello»); `quandoSceglie(voce)`
 * riceve la voce scelta; `limita(voce)` restringe le voci (es. una macchina e un provider);
 * `soloVedi`: le voci non si scelgono (si mettono solo le stelle).
 */
export async function apriSceltaModello({ valore = '', quandoSceglie, tutte = false, limita, soloVedi = false, titolo = 'Scegli il modello', cerca = '' } = {}) {
  const voci = (await gruppiScelta({ tutte })).flatMap((g) => g.voci).filter((v) => !limita || limita(v));
  const perValore = new Map(voci.map((v) => [v.valore, v]));
  const campo = el('input.cerca-modello', { type: 'search', placeholder: 'Cerca: grok, opus, fast, Mac…', 'aria-label': 'Cerca un modello', autocomplete: 'off', value: cerca });
  const elenco = el('div.scelta-elenco', { role: 'listbox', 'aria-label': 'Modelli' });
  const conta = el('small.scelta-conta', { role: 'status' });
  const testa = el('div.foglio-testa', {}, el('h2', {}, titolo));
  const { chiudi, nodo } = apriSovrapposta([testa, el('div.scelta-cerca', {}, icona('cerca', 15), campo, conta), elenco], { classe: 'foglio.scelta-modello', etichetta: titolo });

  const voceNodo = (v, pieno = false) => {
    const pref = preferiti().includes(v.valore);
    const attuale = v.valore === valore;
    const stella = el(`button.stella${pref ? '.accesa' : ''}`, {
      type: 'button', 'aria-pressed': pref ? 'true' : 'false', 'aria-label': `${pref ? 'Togli dai' : 'Aggiungi ai'} preferiti: ${v.testo}`, title: pref ? 'Togli dai preferiti' : 'Aggiungi ai preferiti',
      onclick: async (ev) => { ev.stopPropagation(); await cambiaPreferito(v.valore); disegna(); },
    }, pref ? '★' : '☆');
    const corpo = el(`button.voce-modello${attuale ? '.attuale' : ''}`, {
      type: 'button', role: 'option', 'aria-selected': attuale ? 'true' : 'false', disabled: soloVedi || v.disattivo, dati: { valore: v.valore },
      onclick: async () => { chiudi(); await segnaRecente(v.valore); quandoSceglie?.(v); },
    },
    el('span.voce-nome', {}, pieno ? (eFast(v) ? v.testo.replace(/\s+fast$/i, '') : v.testo) : nomeSenzaFast(v)),
    eFast(v) ? el('span.pastiglia.fast', {}, 'fast') : null,
    altroDettaglio(v) ? el('span.pastiglia', {}, altroDettaglio(v)) : null,
    v.livelli.length ? el('span.pastiglia.pensiero', { title: `Pensiero: ${v.livelli.map(etichettaLivello).join(', ')}` }, `pensiero ${v.livelli.length}`) : null,
    !pieno && v.modello && v.modello.id !== v.etichetta ? el('small.voce-id', {}, v.modello.id) : null,
    attuale ? icona('spunta', 14) : null);
    return el('div.riga-modello', {}, corpo, stella);
  };
  const intesta = (v, n) => el('div.scelta-gruppo', {}, el('span', {}, `${etichettaMacchina(v.macchina)}${inLinea(v.macchina) || !v.macchina?.id ? '' : ' (spenta)'} · ${testoProvider(v)}`), n !== undefined ? el('small', {}, `${n} modell${n === 1 ? 'o' : 'i'}`) : null);

  function disegna() {
    const q = campo.value.trim().toLowerCase();
    const parole = q.split(/\s+/).filter(Boolean);
    const passa = (v) => parole.every((p) => testoRicerca(v).includes(p) || (p === 'fast' && eFast(v)));
    const visibili = voci.filter(passa);
    const blocchi = [];
    if (!q) {
      const pref = preferiti().map((x) => perValore.get(x)).filter(Boolean);
      const rec = recenti().map((x) => perValore.get(x)).filter((v) => v && !pref.includes(v));
      if (pref.length) blocchi.push(el('div.scelta-sezione', {}, el('div.scelta-gruppo', {}, el('span', {}, '★ Preferiti')), ...pref.map((v) => voceNodo(v, true))));
      if (rec.length) blocchi.push(el('div.scelta-sezione', {}, el('div.scelta-gruppo', {}, el('span', {}, 'Recenti')), ...rec.map((v) => voceNodo(v, true))));
    }
    const gruppi = new Map();
    for (const v of visibili) { const k = chiaveGruppo(v); if (!gruppi.has(k)) gruppi.set(k, []); gruppi.get(k).push(v); }
    for (const g of gruppi.values()) {
      const tutteDelGruppo = voci.filter((v) => chiaveGruppo(v) === chiaveGruppo(g[0]));
      const sezione = el('div.scelta-sezione', {}, intesta(g[0], g[0].provider ? contaModelli(tutteDelGruppo) : undefined));
      if (!q && !soloVedi) {
        // senza ricerca: i principali in vista, il resto sotto «Altri modelli»
        const prime = principali(tutteDelGruppo, valore);
        const resto = tutteDelGruppo.filter((v) => !prime.includes(v));
        sezione.append(...prime.map((v) => voceNodo(v)));
        if (resto.length) {
          const aperta = false;
          sezione.append(el('details.famiglia', { open: aperta, dati: { famiglia: 'altri-modelli' } }, el('summary', {}, el('span', {}, 'Altri modelli'), el('small', {}, `${resto.length}`)),
            ...resto.map((v) => voceNodo(v))));
        }
      } else if (g.length > SOGLIA_FAMIGLIE) {
        const fuori = g.filter((v) => !v.modello);
        sezione.append(...fuori.map((v) => voceNodo(v)));
        const fam = new Map();
        for (const v of g.filter((x) => x.modello)) { const f = famiglia(v); if (!fam.has(f.id)) fam.set(f.id, { f, voci: [] }); fam.get(f.id).voci.push(v); }
        const ordine = [...FAMIGLIE.map((x) => x[0]), 'altri'];
        for (const { f, voci: vf } of [...fam.values()].sort((a, b) => ordine.indexOf(a.f.id) - ordine.indexOf(b.f.id))) {
          const aperta = !!q || vf.some((v) => v.valore === valore);
          sezione.append(el('details.famiglia', { open: aperta, dati: { famiglia: f.id } }, el('summary', {}, el('span', {}, f.nome), el('small', {}, `${vf.length}`)), ...vf.map((v) => voceNodo(v))));
        }
      } else sezione.append(...g.map((v) => voceNodo(v)));
      blocchi.push(sezione);
    }
    if (!visibili.length) blocchi.push(el('p.nota.vuota', {}, `Nessun modello per «${campo.value.trim()}».`));
    conta.textContent = q ? `${visibili.filter((v) => v.modello).length} trovati` : `${contaModelli(voci)} modelli`;
    elenco.replaceChildren(...blocchi);
  }
  campo.addEventListener('input', disegna);
  campo.addEventListener('keydown', (ev) => {
    if (ev.key === 'Enter') { ev.preventDefault(); elenco.querySelector('.voce-modello:not([disabled])')?.click(); }
    if (ev.key === 'ArrowDown') { ev.preventDefault(); elenco.querySelector('.voce-modello:not([disabled])')?.focus(); }
  });
  disegna();
  setTimeout(() => { campo.focus(); elenco.querySelector('.voce-modello.attuale')?.scrollIntoView({ block: 'center' }); }, 0);
  return { chiudi, nodo };
}

/**
 * Il bottone che mostra la scelta attuale e apre il foglio. `testo` è quello già calcolato
 * (anche per una scelta che oggi non c'è fra le voci, es. Mac spento).
 */
export function bottoneScelta({ testo, valore, quandoSceglie, tutte = false, etichetta = 'Macchina, provider e modello', classe = '' }) {
  return el(`button.tasto-scelta${classe ? `.${classe}` : ''}`, {
    type: 'button', 'aria-haspopup': 'dialog', 'aria-label': `${etichetta}: ${testo}. Cambia`, title: testo,
    onclick: () => apriSceltaModello({ valore, quandoSceglie, tutte }),
  }, el('span.testo-scelta', {}, testo), icona('giu', 12));
}

/** La tendina «Pensiero» con le etichette italiane; vuota se il modello non ha livelli. */
export function tendinaPensiero(livelli, attuale, f, { etichetta = 'Livello di pensiero' } = {}) {
  if (!livelli?.length) return null;
  const sel = el('select', { 'aria-label': etichetta, onchange: (ev) => f(ev.target.value || null) },
    el('option', { value: '' }, 'predefinito'),
    ...livelli.map((l) => el('option', { value: l, selected: l === attuale }, etichettaLivello(l))));
  if (attuale && !livelli.includes(attuale)) sel.prepend(el('option', { value: attuale, selected: true }, etichettaLivello(attuale)));
  return el('label.pensiero', { title: 'Livello di pensiero del modello' }, el('span', {}, 'Pensiero'), sel);
}

/** Riepilogo «Mac · Cursor · 265 modelli» con «mostra tutti» (Computer, Squadra). */
export async function riepilogoModelli({ macchina } = {}) {
  const gruppi = await gruppiScelta({ tutte: true });
  const righe = [];
  for (const g of gruppi) {
    if (macchina && g.macchina.id !== macchina.id) continue;
    const perProvider = new Map();
    for (const v of g.voci) if (v.provider) { if (!perProvider.has(v.provider.id)) perProvider.set(v.provider.id, []); perProvider.get(v.provider.id).push(v); }
    for (const vs of perProvider.values()) {
      const v = vs[0];
      const n = contaModelli(vs);
      righe.push(el('div.riga-conteggio', {},
        el('span', {}, macchina ? '' : `${etichettaMacchina(g.macchina)} · `, el('strong', {}, testoProvider(v)), ` · ${n} modell${n === 1 ? 'o' : 'i'}`),
        n ? el('button.tasto.piccolo', { type: 'button', onclick: () => apriSceltaModello({ tutte: true, soloVedi: true, titolo: `${etichettaMacchina(g.macchina)} · ${testoProvider(v)}`, limita: (x) => x.macchina.id === g.macchina.id && x.provider?.id === v.provider.id }) }, 'mostra tutti') : null));
    }
  }
  return righe;
}
