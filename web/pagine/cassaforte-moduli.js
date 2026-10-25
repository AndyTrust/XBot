// I moduli della cassaforte (CONTRATTO-2D sez. 1): accesso (con PIN e passphrase), carta (Luhn),
// contatto, segreto (con gli assistenti che lo ricevono), e l'importazione da CSV con il riepilogo.

import { api, testoErrore } from '../api.js';
import { icona } from '../icone.js';
import { stato } from '../stato.js';
import { campo, foglio, nota } from '../ui.js';
import { avvisa, el } from '../util.js';

export const NOMI_GENERE = { accesso: 'Accesso', carta: 'Carta', contatto: 'Contatto', segreto: 'Segreto' };

/** Un campo mascherato con l'occhio per mostrarlo. */
export function campoSegreto(etichetta, name, { valore = '', placeholder = '', required = false, autocomplete = 'new-password' } = {}, largo = false) {
  const input = el('input', { name, type: 'password', value: valore, placeholder, required, autocomplete, spellcheck: 'false' });
  const occhio = el('button.tasto-icona.piccolo', { type: 'button', 'aria-label': `Mostra ${etichetta}`, onclick: () => {
    const nascosto = input.type === 'password';
    input.type = nascosto ? 'text' : 'password';
    occhio.setAttribute('aria-label', `${nascosto ? 'Nascondi' : 'Mostra'} ${etichetta}`);
    occhio.replaceChildren(icona(nascosto ? 'occhiochiuso' : 'occhio', 15));
  } }, icona('occhio', 15));
  return el(`label.campo-modulo${largo ? '.largo' : ''}`, {}, etichetta, el('span.con-occhio', {}, input, occhio));
}

/** In modifica: il tasto «Togli» cancella un campo cifrato già salvato (il server riceve null). */
function togli(nome, etichetta, mod) {
  if (!mod) return null;
  const nascosto = el('input', { type: 'hidden', name: `togli:${nome}`, value: '' });
  const tasto = el('button.tasto.piccolo', { type: 'button', 'aria-pressed': 'false', onclick: () => {
    const si = nascosto.value !== '1';
    nascosto.value = si ? '1' : '';
    tasto.setAttribute('aria-pressed', String(si));
    tasto.textContent = si ? `${etichetta} verrà tolto (annulla)` : `Togli ${etichetta}`;
  } }, `Togli ${etichetta}`);
  return el('div.campo-modulo.largo', {}, nascosto, tasto);
}

export function luhn(numero) {
  const c = numero.replace(/\D/g, '').split('').reverse().map(Number);
  if (c.length < 12 || c.length > 19) return false;
  return c.reduce((s, d, i) => s + (i % 2 ? (d * 2 > 9 ? d * 2 - 9 : d * 2) : d), 0) % 10 === 0;
}

const righe = (t) => (t || '').split('\n').map((x) => x.trim()).filter(Boolean);

/** Il foglio di una voce nuova o da modificare. `extra.assistenti` preseleziona chi riceve un segreto. */
export function foglioVoce(genere, voce = null, extra = {}, dopo = () => {}) {
  const rif = voce?.riferimento || {};
  const mod = !!voce;
  const ph = mod ? 'invariato se lasci vuoto' : '';
  let corpo;
  if (genere === 'accesso') {
    corpo = [
      campo('Titolo', { name: 'titolo', valore: voce?.titolo || '', required: true, maxlength: 120, placeholder: 'es. Gestionale' }, true),
      campo('Nome utente o email', { name: 'utente', valore: rif.utente || '', autocomplete: 'off' }),
      campoSegreto('Password', 'password', { placeholder: ph, required: !mod }),
      campo('Siti (uno per riga)', { name: 'siti', valore: (voce?.siti || []).join('\n'), rows: 2, placeholder: 'https://…', spellcheck: 'false' }, true),
      campoSegreto('PIN (facoltativo)', 'pin', { placeholder: ph }),
      togli('pin', 'il PIN', mod),
      campoSegreto('Passphrase (facoltativa)', 'passphrase', { placeholder: mod ? ph : 'frase di sblocco o di recupero' }),
      togli('passphrase', 'la passphrase', mod),
      campo('Note', { name: 'note', rows: 2, spellcheck: 'false', placeholder: mod ? 'invariate se lasci vuoto' : '' }, true),
      togli('note', 'le note', mod),
    ].filter(Boolean);
  } else if (genere === 'carta') {
    corpo = [
      campoSegreto('Numero della carta', 'numero', { placeholder: mod ? `•••• ${rif.last4 || ''} (${ph})` : '', required: !mod, autocomplete: 'off' }, true),
      campo('Scadenza (MM/AA)', { name: 'scadenza', valore: rif.exp_month ? `${String(rif.exp_month).padStart(2, '0')}/${String(rif.exp_year).slice(-2)}` : '', placeholder: '12/27', required: true, inputmode: 'numeric' }),
      campoSegreto('Codice (facoltativo)', 'cvc', { placeholder: ph }),
      togli('cvc', 'il codice', mod),
      campo('Intestatario', { name: 'intestatario', valore: rif.intestatario || '', autocomplete: 'off' }),
      campo('Nome breve', { name: 'nome_breve', valore: rif.nome_breve || voce?.titolo || '', placeholder: 'es. Visa aziendale' }),
      campo('Indirizzo di fatturazione', { name: 'indirizzo', rows: 2, spellcheck: 'false', placeholder: mod ? ph : '' }, true),
      togli('indirizzo', 'l\'indirizzo', mod),
      nota('Il bot riceve numero e codice della carta solo dopo il tuo sì in chat, una volta per uso.'),
    ];
  } else if (genere === 'contatto') {
    const ind = rif.indirizzo || {};
    corpo = [
      campo('Nome', { name: 'nome', valore: rif.nome || voce?.titolo || '', required: true }, true),
      campo('Email', { name: 'email', type: 'email', valore: rif.email || '' }),
      campo('Telefono', { name: 'telefono', type: 'tel', valore: rif.telefono || '' }),
      campo('Via', { name: 'via', valore: ind.via || '' }, true),
      campo('CAP', { name: 'cap', valore: ind.cap || '', inputmode: 'numeric' }),
      campo('Città', { name: 'citta', valore: ind.citta || '' }),
      nota('I contatti sono in chiaro per scelta: gli assistenti li usano per compilare al posto tuo.'),
    ];
  } else {
    const scelti = new Set(voce?.assistenti || extra.assistenti || []);
    corpo = [
      campo('Nome della variabile', { name: 'variabile', valore: rif.variabile || voce?.titolo || '', required: true, placeholder: 'ODOO_API_KEY (MAIUSCOLE_E_UNDERSCORE)', autocomplete: 'off', spellcheck: 'false' }, true),
      campoSegreto('Valore', 'valore', { placeholder: ph, required: !mod }, true),
      campo('Descrizione', { name: 'descrizione', valore: rif.descrizione || '' }, true),
      el('fieldset.caselle.largo', {}, el('legend', {}, 'Assistenti che lo ricevono come variabile d\'ambiente'),
        ...[...stato.assistenti.values()].filter((a) => !a.archiviato_ms).map((a) => el('label.casella', {},
          el('input', { type: 'checkbox', name: `assistente:${a.id}`, checked: scelti.has(a.id) }), a.nome))),
    ];
  }
  foglio({
    titolo: mod ? `${NOMI_GENERE[genere]}: ${voce.titolo}` : `${genere === 'carta' ? 'Nuova' : 'Nuovo'} ${NOMI_GENERE[genere].toLowerCase()}`,
    conferma: mod ? 'Salva' : 'Aggiungi',
    corpo,
    invia: async (m) => {
      const v = Object.fromEntries(new FormData(m));
      const dati = datiDa(genere, v, m, mod);
      try {
        if (mod) await api.modificaVoce(voce.id, dati); else await api.creaVoce(dati);
      } catch (e) { throw new Error(testoErrore(e)); }
      avvisa(mod ? 'Salvato' : 'Aggiunto alla cassaforte');
      dopo();
    },
  });
}

function datiDa(genere, v, modulo, mod) {
  const campi = {};
  const metti = (k, x) => {
    if (mod && v[`togli:${k}`] === '1') campi[k] = null;
    else if (x !== undefined && x !== '') campi[k] = x;
  };
  if (genere === 'accesso') {
    ['password', 'pin', 'passphrase', 'note'].forEach((k) => metti(k, v[k]));
    return { genere, titolo: v.titolo.trim(), siti: righe(v.siti), riferimento: { utente: v.utente.trim() }, campi };
  }
  if (genere === 'carta') {
    const numero = (v.numero || '').replace(/[\s-]/g, '');
    if (numero && /\D/.test(numero)) throw new Error('Il numero della carta vuole solo cifre (spazi e trattini vanno bene).');
    if (numero && (numero.length < 12 || numero.length > 19)) throw new Error(`Il numero ha ${numero.length} cifre: una carta ne ha da 12 a 19.`);
    if (numero && !luhn(numero)) throw new Error('Il numero della carta non è valido: una cifra è sbagliata o scambiata, controlla.');
    if (!/^\d{2}\/\d{2}$/.test((v.scadenza || '').trim())) throw new Error('La scadenza si scrive MM/AA, per esempio 12/27.');
    if (!mod && !numero) throw new Error('Serve il numero della carta.');
    const [mm, aa] = (v.scadenza || '').split('/');
    const mese = Number(mm);
    if (!(mese >= 1 && mese <= 12)) throw new Error('La scadenza vuole il mese fra 01 e 12.');
    metti('numero', numero); metti('cvc', v.cvc); metti('indirizzo', v.indirizzo);
    const nome = v.nome_breve.trim();
    return { genere, titolo: nome || `Carta •••• ${numero.slice(-4)}`, siti: [], campi,
      riferimento: { exp_month: mese, exp_year: 2000 + Number(aa), intestatario: v.intestatario.trim(), nome_breve: nome } };
  }
  if (genere === 'contatto') {
    return { genere, titolo: v.nome.trim(), siti: [], campi: {},
      riferimento: { nome: v.nome.trim(), email: v.email.trim(), telefono: v.telefono.trim(), indirizzo: { via: v.via.trim(), cap: v.cap.trim(), citta: v.citta.trim() } } };
  }
  const variabile = (v.variabile || '').trim();
  if (!/^[A-Z][A-Z0-9_]*$/.test(variabile)) throw new Error('Il nome della variabile vuole MAIUSCOLE_E_UNDERSCORE: comincia con una lettera maiuscola, poi maiuscole, cifre o _ (es. ODOO_API_KEY).');
  if (/^(PATH|HOME|LD_PRELOAD|LD_LIBRARY_PATH)$|^(DYLD_|XBOT_|ANTHROPIC_|CLAUDE_)/.test(variabile)) throw new Error(`«${variabile}» è un nome riservato al sistema: scegline un altro.`);
  metti('valore', v.valore);
  const assistenti = [...modulo.querySelectorAll('input[name^="assistente:"]')].filter((c) => c.checked).map((c) => c.name.slice(11));
  return { genere, titolo: v.variabile.trim(), siti: [], campi, assistenti, riferimento: { variabile: v.variabile.trim(), descrizione: v.descrizione.trim() } };
}

/** Importa da CSV (Bitwarden, LastPass, Firefox, 1Password, Safari, Chrome/Edge) con il riepilogo. */
export function foglioImporta(dopo = () => {}) {
  const file = el('input', { type: 'file', name: 'file', accept: '.csv,text/csv', required: true });
  const f = foglio({
    titolo: 'Importa da CSV', conferma: 'Importa', unaColonna: true,
    corpo: [
      el('p.largo', {}, 'Esporta le password dal vecchio programma in CSV e scegli il file: il formato si riconosce dall\'intestazione (Bitwarden, LastPass, Firefox, 1Password, Safari, Chrome, Edge).'),
      el('label.campo-modulo.largo', {}, 'File CSV', file),
      nota('Dopo l\'importazione cancella il file CSV dal computer: contiene le password in chiaro.'),
    ],
    invia: async () => {
      if (!file.files[0]) throw new Error('Scegli un file.');
      let r;
      try { r = await api.importaCsv(file.files[0]); } catch (e) { throw new Error(testoErrore(e)); }
      const errori = r?.errori || [];
      dopo();
      const formato = r?.formato ? ` (${r.formato})` : '';
      avvisa(`Importati ${r?.importati ?? 0}, saltati ${r?.saltati ?? 0}${formato}${errori.length ? ` · ${errori.length} righe con errori` : ''}`, 6000);
      if (errori.length) console.info('Importazione CSV, righe con errori:', errori.slice(0, 20));
    },
  });
  const tasto = f.modulo.querySelector('button[type=submit]');
  tasto.disabled = true;
  file.addEventListener('change', () => { tasto.disabled = !file.files.length; });
}
