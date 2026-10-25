// Le richieste all'utente (Layout sez. 6): permesso, revisione del comando, modulo, segreto,
// cassaforte, domanda, spostamento. La stessa scheda vale in chat e nella pagina Permessi.
// Risposta: POST /richieste/{id}/risolvi {esito, sempre?, comando_approvato?, valori?}.

import { api, testoErrore } from './api.js';
import { icona } from './icone.js';
import { assistenteDi, bus } from './stato.js';
import { avvisa, el, quandoBreve, testoDa } from './util.js';

/** Porta a una forma sola una richiesta presa da una riga (corpo) o da GET /richieste. */
export function normalizza(r) {
  let carico = r.carico ?? r.carico_json ?? {};
  if (typeof carico === 'string') { try { carico = JSON.parse(carico); } catch { carico = { testo: carico }; } }
  let risoluzione = r.risoluzione ?? r.risoluzione_json ?? null;
  if (typeof risoluzione === 'string') { try { risoluzione = JSON.parse(risoluzione); } catch { risoluzione = null; } }
  return { id: r.richiesta_id || r.id, tipo: r.tipo, carico: carico || {}, stato: r.stato || 'aperta', risoluzione,
    assistente_id: r.assistente_id, sessione_id: r.sessione_id, creata_ms: r.creata_ms, scade_ms: r.scade_ms,
    risolta_ms: r.risolta_ms ?? risoluzione?.risolta_ms ?? r.aggiornata_ms ?? null };
}

const TITOLI = {
  permesso_locale: 'Permesso', revisione_comando: 'Revisione del comando', modulo: 'Modulo', segreto: 'Segreto',
  cassaforte: 'Cassaforte', domanda: 'Domanda', collegamento: 'Collegamento', spostamento_sessione: 'Spostamento',
};

function esitoTesto(r) {
  const ris = r.risoluzione || {};
  if (r.stato === 'scaduta') return 'Scaduta senza risposta';
  if (r.stato === 'ritirata') return 'Ritirata dall\'assistente';
  if (r.stato === 'negata') return 'Negata';
  if (r.tipo === 'domanda' && ris.valori?.scelta) return `Risposto: ${ris.valori.scelta}`;
  if (r.tipo === 'spostamento_sessione') return 'Continua sull\'altra macchina';
  if (r.carico?.azione === 'invio_posta') return r.carico.pec ? 'Via libera alla PEC' : 'Via libera alla mail';
  if (ris.comando_approvato) return `Consentito con il comando modificato: ${ris.comando_approvato}`;
  if (ris.valori?.salva) return 'Dato e salvato in cassaforte';
  if (r.tipo === 'modulo' && (r.carico ?? r.carico_json)?.senza_salva) return 'Salvato cifrato';
  return ris.sempre ? 'Consentito sempre' : 'Consentito una volta';
}

const ora = (ms) => (ms ? ` alle ${new Date(ms).toLocaleTimeString('it-IT', { hour: '2-digit', minute: '2-digit' })}` : '');
/** Il titolo dice cosa è successo davvero; per la revisione del comando dipende dallo stato. */
function titoloDi(r) {
  if (r.carico?.azione === 'invio_posta') return r.carico.pec ? 'Invio di una PEC' : 'Invio di una mail';
  if (r.tipo !== 'revisione_comando') return TITOLI[r.tipo] || r.tipo || 'Richiesta';
  switch (r.stato) {
    case 'approvata': return `Approvato${ora(r.risolta_ms)}`;
    case 'negata': return `Negato${ora(r.risolta_ms)}`;
    case 'ritirata': return 'Ritirato dall\'assistente';
    case 'scaduta': return 'Scaduto';
    default: return 'Comando da approvare';
  }
}

/** Il box del segreto chiesto in chat (2026-10-25): chiave, nome, «salvato cifrato, mai in chat»,
 *  campo «Incolla il valore», Salva e Non ora. Se il server lo chiede (`ambito_scegli`) si sceglie se
 *  vale solo per questo assistente o per tutti; Mac e VPS lo ricevono comunque dalla stessa cassaforte. */
function schedaSegreto(r, manda, errore) {
  const c = r.carico;
  const nome = assistenteDi(r.assistente_id).nome || 'questo assistente';
  const campi = (c.campi || []).map((f) => {
    const segreto = f.tipo === 'segreto';
    const input = el('input.richiesta-campo', { name: f.nome, required: !!f.obbligatorio, 'aria-label': f.etichetta || f.nome,
      type: segreto ? 'password' : 'text', autocomplete: segreto ? 'new-password' : 'off', spellcheck: 'false',
      placeholder: segreto ? 'Incolla il valore · resta cifrato in XBot' : (f.etichetta || f.nome), ...(f.valore ? { value: f.valore } : {}) });
    return segreto ? input : el('label.campo-modulo', {}, f.etichetta || f.nome, input);
  });
  let ambito = c.ambito || 'assistente';
  const dove = el('span');
  const scambia = el('button.tasto-link', { type: 'button' });
  const ridisegnaDove = () => {
    dove.textContent = ambito === 'tutti' ? 'Salvato cifrato in XBot per tutti gli assistenti, mai mostrato in chat' : `Salvato cifrato in XBot per ${nome}, mai mostrato in chat`;
    scambia.textContent = ambito === 'tutti' ? `Solo per ${nome}` : 'Usa per tutti gli assistenti';
  };
  scambia.onclick = () => { ambito = ambito === 'tutti' ? 'assistente' : 'tutti'; ridisegnaDove(); };
  ridisegnaDove();
  const segreti = (c.campi || []).filter((f) => f.tipo === 'segreto');
  const tra = /«([^»]+)»/.exec(c.titolo || '')?.[1];
  const generica = (x) => !x || /^(valore|password|token)$/i.test(x);
  const titolo = c.nome_segreto || (segreti.length === 1 && !generica(segreti[0].etichetta) ? segreti[0].etichetta : tra || c.titolo || 'Credenziali');
  const nodo = el(`div.richiesta.segreto-box.${r.stato}`, { role: 'group', 'aria-label': `${titolo}: valore protetto`, dati: { richiesta: r.id } });
  const salva = el('button.tasto.primario', { type: 'button', onclick: () => {
    const valori = {};
    for (const i of nodo.querySelectorAll('[name]')) {
      if (i.required && !i.value.trim()) { i.focus(); errore.textContent = `Serve «${i.getAttribute('aria-label')}».`; errore.hidden = false; return; }
      valori[i.name] = i.value;
    }
    if (c.ambito_scegli) valori.ambito = ambito;
    manda('approvata', { valori });
  } }, 'Salva');
  const invio = (ev) => { if (ev.key === 'Enter' && ev.target.type === 'password') { ev.preventDefault(); salva.click(); } };
  nodo.addEventListener('keydown', invio);
  nodo.append(
    el('div.segreto-testa', {}, el('span.segreto-icona', {}, icona('chiave', 18)),
      el('div.segreto-testi', {}, el('strong', {}, titolo),
        el('small', {}, dove, c.ambito_scegli ? ' · ' : null, c.ambito_scegli ? scambia : null)),
      el('button.tasto-link.segreto-non-ora', { type: 'button', onclick: () => manda('negata') }, 'Non ora')),
    c.aiuto ? el('small.aiuto-modulo', { style: 'white-space:pre-line' }, c.aiuto) : null,
    ...campi.filter((x) => x.tagName === 'LABEL'),
    el('div.segreto-riga', {}, ...campi.filter((x) => x.tagName !== 'LABEL'), salva),
    errore);
  return nodo;
}

/** La scheda di una richiesta. `quandoRisolta(r)` parte dopo la risposta del server. */
export function schedaRichiesta(grezza, { conAssistente = false, quandoRisolta } = {}) {
  const r = normalizza(grezza);
  const c = r.carico;
  const nome = assistenteDi(r.assistente_id).nome || 'L\'assistente';
  const nodo = el(`div.richiesta.${r.stato}`, { role: 'group', 'aria-label': `${TITOLI[r.tipo] || 'Richiesta'} di ${nome}`, dati: { richiesta: r.id } });
  const testa = el('div.richiesta-testa', {}, icona(r.tipo === 'domanda' ? 'info' : r.tipo === 'cassaforte' || r.tipo === 'segreto' ? 'chiave' : 'scudo', 16),
    el('strong', {}, titoloDi(r)), conAssistente ? el('span', {}, `· ${nome}`) : null,
    r.creata_ms ? el('small', {}, quandoBreve(r.creata_ms)) : null);
  const corpo = el('div.richiesta-corpo');
  const tasti = el('div.richiesta-tasti');
  const errore = el('p.errore-modulo', { role: 'alert', hidden: true });

  const manda = async (esito, extra = {}) => {
    for (const t of nodo.querySelectorAll('button,input,textarea')) t.disabled = true;
    errore.hidden = true;
    try {
      await api.risolvi(r.id, { esito, ...extra });
      const nuova = { ...r, stato: esito, risolta_ms: Date.now(), risoluzione: { esito, ...extra } };
      nodo.replaceWith(schedaRichiesta(nuova, { conAssistente, quandoRisolta }));
      bus.emetti('richieste');
      quandoRisolta?.(nuova);
    } catch (e) {
      for (const t of nodo.querySelectorAll('button,input,textarea')) t.disabled = false;
      errore.textContent = testoErrore(e);
      errore.hidden = false;
    }
  };
  const tasto = (testo, f, primario = false) => el(`button.tasto.piccolo${primario ? '.primario' : ''}`, { type: 'button', onclick: f }, testo);
  const treTasti = (extra = () => ({})) => [
    tasto('Consenti una volta', () => manda('approvata', extra()), true),
    tasto('Sempre', () => manda('approvata', { sempre: true, ...extra() })),
    tasto('No', () => manda('negata')),
  ];
  const codice = (t) => (t ? el('code.richiesta-codice', {}, t) : null);

  switch (r.tipo) {
    case 'permesso_locale':
      if (c.azione === 'invio_posta') {
        // La mail si manda solo con il sì dell'utente: due tasti, mai «Sempre».
        const elenco = (v) => (Array.isArray(v) ? v.join(', ') : '');
        corpo.append(el('p', {}, `${nome} vuole mandare questa ${c.pec ? 'PEC' : 'mail'}:`),
          el('small', {}, `Da: ${c.da || ''}`), el('small', {}, `A: ${elenco(c.a)}`),
          c.cc?.length ? el('small', {}, `Cc: ${elenco(c.cc)}`) : null, c.ccn?.length ? el('small', {}, `Ccn: ${elenco(c.ccn)}`) : null,
          el('small', {}, `Oggetto: ${c.oggetto || ''}`), el('pre.richiesta-codice', {}, c.testo || ''),
          c.testo_troncato ? el('small', {}, 'Testo accorciato nella richiesta.') : null,
          c.descrizione ? el('small', {}, c.descrizione) : null);
        tasti.append(tasto(c.pec ? 'Invia la PEC' : 'Invia la mail', () => manda('approvata'), true), tasto('No', () => manda('negata')));
        break;
      }
      corpo.append(el('p', {}, `${nome} vuole ${c.comando ? 'eseguire' : `usare ${c.strumento || 'uno strumento'}`}${c.macchina ? ` su ${c.macchina}` : ''}:`),
        codice(c.comando || testoDa(c.input || c.descrizione || '')),
        c.cartella ? el('small', {}, `Cartella: ${c.cartella}`) : null, c.descrizione && c.comando ? el('small', {}, c.descrizione) : null);
      tasti.append(...treTasti());
      break;
    case 'revisione_comando': {
      const modifica = el('input.richiesta-campo', { value: c.comando || '', 'aria-label': 'Comando da approvare', hidden: true });
      // «regola» c'è solo se il carico la porta (oggi le regole consentono o negano da sole, senza richiesta)
      const frase = c.regola ? `La regola di revisione «${c.regola}» ha fermato:` : r.stato === 'aperta' ? `${nome} vuole eseguire${c.macchina ? ` su ${c.macchina}` : ''}:` : 'Comando:';
      corpo.append(el('p', {}, frase), codice(c.comando), c.descrizione ? el('small', {}, c.descrizione) : null, modifica);
      tasti.append(...treTasti(() => (modifica.hidden || modifica.value === c.comando ? {} : { comando_approvato: modifica.value })),
        tasto('Modifica il comando', (ev) => { modifica.hidden = false; modifica.focus(); ev.currentTarget.remove(); }));
      break;
    }
    case 'modulo': {
      if (c.senza_salva && r.stato === 'aperta') return schedaSegreto(r, manda, errore);
      const campi = (c.campi || []).map((f) => {
        const attr = { name: f.nome, required: !!f.obbligatorio, 'aria-label': f.etichetta || f.nome };
        const segreto = f.tipo === 'segreto';
        const input = f.tipo === 'area' ? el('textarea.richiesta-campo', { ...attr, rows: 3 })
          : el('input.richiesta-campo', { ...attr, type: segreto ? 'password' : 'text', autocomplete: segreto ? 'new-password' : 'off', spellcheck: 'false', ...(f.valore ? { value: f.valore } : {}) });
        return el('label.campo-modulo', {}, f.etichetta || f.nome, input);
      });
      // `senza_salva`: i valori vanno già cifrati dove servono (cassaforte o collegamento), il bot non li vede.
      const salva = c.senza_salva ? null : el('input', { type: 'checkbox' });
      corpo.append(c.titolo ? el('p', {}, el('strong', {}, c.titolo)) : null,
        c.aiuto ? el('small.aiuto-modulo', { style: 'white-space:pre-line' }, c.aiuto) : null, ...campi,
        salva ? el('label.casella', {}, salva, 'Salva nella cassaforte') : el('small', {}, 'Resta cifrato su XBot: l\'assistente non vede il valore.'));
      tasti.append(tasto(c.senza_salva ? 'Salva' : 'Invia', () => {
        const valori = {};
        for (const i of corpo.querySelectorAll('[name]')) {
          if (i.required && !i.value.trim()) { i.focus(); errore.textContent = `Serve «${i.getAttribute('aria-label')}».`; errore.hidden = false; return; }
          valori[i.name] = i.value;
        }
        if (salva?.checked) valori.salva = true;
        manda('approvata', { valori });
      }, true), tasto('Annulla', () => manda('negata')));
      break;
    }
    case 'segreto': {
      const v = el('input.richiesta-campo', { type: 'password', autocomplete: 'off', 'aria-label': c.variabile || 'Valore' });
      corpo.append(el('p', {}, `Serve ${c.variabile || 'un segreto'}${c.motivo ? ` per ${c.motivo.charAt(0).toLowerCase()}${c.motivo.slice(1)}` : ''}.`), v);
      const conValore = (salva) => () => { if (!v.value) { v.focus(); return; } manda('approvata', { valori: { valore: v.value, ...(salva ? { salva: true } : {}) } }); };
      tasti.append(tasto('Dai una volta', conValore(false), true), tasto('Salva in cassaforte', conValore(true)), tasto('No', () => manda('negata')));
      break;
    }
    case 'cassaforte':
      corpo.append(el('p', {}, `${nome} vuole usare «${c.titolo || 'una voce della cassaforte'}».`), c.motivo ? el('small', {}, `Motivo: ${c.motivo}`) : null,
        el('small', {}, 'Il valore va solo nel processo del provider, mai nella chat.'));
      tasti.append(...treTasti());
      break;
    case 'domanda': {
      const altro = el('input.richiesta-campo', { placeholder: 'Scrivi la risposta', 'aria-label': 'Altra risposta', hidden: true,
        onkeydown: (ev) => { if (ev.key === 'Enter' && altro.value.trim()) { ev.preventDefault(); manda('approvata', { valori: { scelta: altro.value.trim() } }); } } });
      corpo.append(el('p', {}, c.testo || c.domanda || ''), altro);
      tasti.append(...(c.opzioni || []).map((o, i) => tasto(o, () => manda('approvata', { valori: { scelta: o } }), i === 0)),
        tasto('Altro…', (ev) => { altro.hidden = false; altro.focus(); ev.currentTarget.textContent = 'Invia'; ev.currentTarget.onclick = () => altro.value.trim() && manda('approvata', { valori: { scelta: altro.value.trim() } }); }));
      break;
    }
    case 'spostamento_sessione':
      corpo.append(el('p', {}, `Continuare su ${c.macchina || 'un\'altra macchina'}? La sessione si riapre con il riassunto.`));
      tasti.append(tasto('Continua', () => manda('approvata'), true), tasto('Resta qui', () => manda('negata')));
      break;
    default:
      corpo.append(el('pre.richiesta-codice', {}, testoDa(c)));
      tasti.append(tasto('Consenti', () => manda('approvata'), true), tasto('No', () => manda('negata')));
  }
  if (r.stato !== 'aperta') {
    for (const i of corpo.querySelectorAll('input,textarea,label.casella')) i.remove();
    return el(`div.richiesta.chiusa.${r.stato}`, { role: 'group', dati: { richiesta: r.id } }, testa, corpo,
      el('div.richiesta-esito', {}, icona(r.stato === 'approvata' ? 'spunta' : 'chiudi', 14), esitoTesto(r)));
  }
  nodo.append(testa, corpo, tasti, errore);
  return nodo;
}

/** Le richieste aperte dal server (vuoto se la rotta manca). */
export async function richiesteAperte() {
  try {
    const r = await api.richieste('aperta');
    return (Array.isArray(r) ? r : r?.richieste || []).map(normalizza);
  } catch (e) {
    if (e.codice !== 'rete') console.warn('richieste', e.codice);
    return [];
  }
}

export function avvisaErrore(e) { avvisa(testoErrore(e)); }
