// La barra laterale (Layout sez. 1.1, rifatta in 2.H-bis): cerca, nuovo, i fissati
// in una riga orizzontale di avatar grandi con il nome sotto, sezioni richiudibili con il conteggio,
// righe con avatar grande animato e solo il nome (niente anteprima, 2026-10-25), un puntino
// sull'avatar (blu non letti, arancio «tocca a te», verde pulsante al lavoro), account in basso,
// Collegamenti con il numero. Ricaricata a ogni frame turno e ogni 30 s.

import { api, testoErrore } from './api.js';
import { avatarVivo } from './avatar.js';
import { icona, iconaIn } from './icone.js';
import { apriMenu, chiediConferma, chiediTesto } from './menu.js';
import { assistenteDi, bus, caricaAssistenti, caricaBarra, chiaveBozza, CHIAVI, impostazione, scriviImpostazione, stato } from './stato.js';
import { $, avvisa, el, locale } from './util.js';

let azioni = {};

export function preparaBarra(a) {
  azioni = a; // {apri, nuovo, cerca, impostazioni, esci}
  iconaIn($('#tasto-cerca'), 'cerca');
  iconaIn($('#tasto-nuovo'), 'piu');
  $('#tasto-cerca').onclick = () => azioni.cerca();
  $('#tasto-nuovo').onclick = () => azioni.nuovo();
  $('#tasto-account').onclick = (ev) => menuAccount(ev.currentTarget);
  $('#tasto-collegamenti').onclick = () => azioni.collegamenti();
  for (const e of ['barra', 'assistenti', 'bozza', 'letto']) bus.su(e, () => disegnaBarra());
  bus.su('letto', () => ricarica());
  // righe nuove nella sessione aperta: l'anteprima e i pallini cambiano (una ricarica ogni 500 ms al massimo)
  let timerRighe;
  bus.su('flusso:righe', () => { clearTimeout(timerRighe); timerRighe = setTimeout(ricarica, 500); });
  // il frame SSE `barra` (turno, attese, letto) rilegge subito; il sondaggio ogni 30 s, solo con la
  // pagina visibile, resta come rete di sicurezza: il frame arriva solo per la sessione aperta
  bus.su('flusso:barra', () => ricarica());
  setInterval(() => { if (document.visibilityState === 'visible') ricarica(); }, 30000);
  bus.su('pagina:visibile', () => ricarica()); // 2.L difetto 11: al ritorno si rilegge subito
}

let inCorso = null;
/** Ricarica la barra dal server (una alla volta). */
export function ricarica() {
  if ($('#finestra').hidden) return Promise.resolve(); // pagina di accesso aperta: niente sondaggio
  if (!inCorso) inCorso = caricaBarra().catch(() => {}).finally(() => { inCorso = null; });
  return inCorso;
}

/** Vero se l'ultima sessione dell'assistente ha una bozza non mandata (una matita accanto al nome). */
function haBozza(id) {
  const ultima = locale.leggi(`ultima_sessione.${id}`);
  const bozza = ultima ? locale.leggi(chiaveBozza(ultima), '') : '';
  return !!(bozza && bozza.trim());
}

function etichettaDi(a, v) {
  return [a.nome, v.in_attesa_utente ? 'tocca a te' : '', v.non_letti ? `${v.non_letti} non letti` : '', v.al_lavoro || a.al_lavoro ? 'al lavoro' : '',
    haBozza(v.assistente_id) ? 'bozza' : ''].filter(Boolean).join(', ');
}

function voce(v) {
  const a = assistenteDi(v.assistente_id);
  const corrente = v.assistente_id === stato.assistenteId;
  return el('button.voce', {
    type: 'button', 'aria-current': corrente ? 'true' : 'false', 'aria-label': etichettaDi(a, v), title: a.titolo || a.nome,
    onclick: () => azioni.apri(v.assistente_id),
    oncontextmenu: (ev) => { ev.preventDefault(); menuVoce(ev, v.assistente_id); },
  }, avatarVivo(a, 48, { voce: v, puntino: true, corrente }),
  el('span.voce-nome', {}, a.nome), haBozza(v.assistente_id) ? el('span.voce-bozza', { title: 'Bozza non mandata' }, icona('matita', 13)) : null);
}

/** Tenere premuto su telefono = menu della riga. */
function tieniPremuto(nodo, f) {
  let t;
  nodo.addEventListener('touchstart', (ev) => { t = setTimeout(() => f(ev.touches[0]), 550); }, { passive: true });
  for (const e of ['touchend', 'touchmove', 'touchcancel']) nodo.addEventListener(e, () => clearTimeout(t), { passive: true });
}

export function disegnaBarra() {
  const fissati = $('#fissati');
  const sezioni = $('#sezioni');
  const tutte = stato.barra.flatMap((s) => s.assistenti || []);
  const fis = tutte.filter((v) => v.fissato).slice(0, 9);
  fissati.replaceChildren(...fis.map((v, i) => {
    const a = assistenteDi(v.assistente_id);
    const corrente = v.assistente_id === stato.assistenteId;
    const n = el('button.fissato', {
      type: 'button', title: `${a.nome} (⌘${i + 1})`, 'aria-label': etichettaDi(a, v), 'aria-current': corrente ? 'true' : 'false',
      onclick: () => azioni.apri(v.assistente_id),
      oncontextmenu: (ev) => { ev.preventDefault(); menuVoce(ev, v.assistente_id); },
    }, avatarVivo(a, 60, { voce: v, puntino: true, corrente }), el('span', {}, a.nome));
    tieniPremuto(n, (t) => menuVoce({ clientX: t.clientX, clientY: t.clientY }, v.assistente_id));
    return n;
  }));
  const chiuse = new Set(locale.leggi('sezioni_chiuse', []));
  sezioni.replaceChildren(...stato.barra.filter((s) => (s.assistenti || []).some((v) => !v.fissato) || s.id !== '__non_assegnati__').map((s) => {
    const voci = (s.assistenti || []).filter((v) => !v.fissato);
    if (!voci.length && s.id === '__non_assegnati__') return null;
    const chiusa = chiuse.has(s.id) || s.chiusa === 1;
    const testa = el('button.sezione-testa', {
      type: 'button', 'aria-expanded': chiusa ? 'false' : 'true',
      onclick: () => apriChiudiSezione(s, !chiusa),
      oncontextmenu: (ev) => { ev.preventDefault(); menuSezione(ev, s); },
    }, el('span', {}, s.nome), el('span.conta', {}, voci.length || ''), icona('giu', 14));
    const lista = el('div', { role: 'list', hidden: chiusa }, voci.map((v) => {
      const n = voce(v);
      tieniPremuto(n, (t) => menuVoce({ clientX: t.clientX, clientY: t.clientY }, v.assistente_id));
      return n;
    }));
    tieniPremuto(testa, (t) => menuSezione({ clientX: t.clientX, clientY: t.clientY }, s));
    return el('div.sezione', {}, testa, lista);
  }));
  if (!tutte.length) sezioni.append(el('p', { style: 'padding:12px;color:var(--testo-3);font-size:13px' }, 'Nessun assistente. Crea il primo con ＋.'));
  disegnaAccount();
}

async function apriChiudiSezione(s, chiudi) {
  const chiuse = new Set(locale.leggi('sezioni_chiuse', []));
  if (chiudi) chiuse.add(s.id); else chiuse.delete(s.id);
  locale.scrivi('sezioni_chiuse', [...chiuse]);
  s.chiusa = chiudi ? 1 : 0;
  disegnaBarra();
  try { await api.modificaSezioni([{ id: s.id, chiusa: chiudi ? 1 : 0 }]); } catch { /* resta nel browser */ }
}

function disegnaAccount() {
  const nome = stato.io?.nome || 'Utente';
  $('#tasto-account').replaceChildren(el('span.iniziale', {}, nome.slice(0, 1).toUpperCase()), el('span', {}, nome), icona('giu', 14));
  const accesi = Number(stato.io?.collegamenti_accesi || 0);
  $('#tasto-collegamenti').replaceChildren(icona('presa', 16), el('span', {}, String(accesi)));
}

function menuAccount(ancora) {
  const tema = impostazione(CHIAVI.tema, 'sistema');
  const prossimo = { chiaro: 'scuro', scuro: 'sistema', sistema: 'chiaro' }[tema] || 'chiaro';
  const nomi = { chiaro: 'Chiaro', scuro: 'Scuro', sistema: 'Sistema' };
  const nascosti = [...stato.assistenti.values()].filter((a) => a.nascosto && !a.archiviato_ms);
  apriMenu(ancora, [
    { testo: 'Impostazioni', icona: 'impostazioni', azione: () => azioni.impostazioni() },
    { testo: 'Squadra', icona: 'persona', azione: () => azioni.squadra?.() },
    { testo: 'Collegamenti', icona: 'presa', azione: () => azioni.collegamenti() },
    ...(nascosti.length ? ['-', { titolo: 'Nascosti: tocca per mostrarli' }, ...nascosti.map((a) => ({
      testo: a.nome, icona: 'occhio', azione: async () => {
        try { await api.modificaAssistente(a.id, { nascosto: 0 }); await Promise.all([caricaAssistenti(), caricaBarra()]); avvisa(`${a.nome} di nuovo nella barra`); } catch (e) { avvisa(testoErrore(e)); }
      },
    }))] : []),
    { testo: 'Cambia tema', icona: 'luna', nota: `${nomi[tema]} → ${nomi[prossimo]}`, azione: () => scriviImpostazione(CHIAVI.tema, prossimo).catch((e) => avvisa(testoErrore(e))) },
    '-',
    { testo: 'Esci', icona: 'esci', azione: () => azioni.esci() },
  ]);
}

function menuVoce(ev, id) {
  const a = assistenteDi(id);
  const modifica = async (campi, fatto) => {
    try { await api.modificaAssistente(id, campi); await Promise.all([caricaAssistenti(), caricaBarra()]); if (fatto) avvisa(fatto); } catch (e) { avvisa(testoErrore(e)); }
  };
  const sezioni = stato.barra.filter((s) => s.id !== a.sezione_id);
  apriMenu({ x: ev.clientX, y: ev.clientY }, [
    { titolo: a.nome },
    { testo: 'Apri', icona: 'sessioni', azione: () => azioni.apri(id) },
    { testo: 'Nuova sessione', icona: 'piu', azione: () => azioni.nuovaSessione(id) },
    { testo: 'Impostazioni dell\'assistente…', icona: 'impostazioni', azione: () => azioni.impostazioniAssistente(id) },
    '-',
    { testo: a.fissato ? 'Togli dai fissati' : 'Fissa', icona: 'stella', azione: () => modifica({ fissato: a.fissato ? 0 : 1 }) },
    { testo: 'Segna come letto', icona: 'spunta', azione: async () => { try { await api.letto(id, 0); ricarica(); } catch (e) { avvisa(testoErrore(e)); } } },
    { testo: 'Nascondi', icona: 'chiudi', azione: () => modifica({ nascosto: 1 }, `${a.nome} nascosto`) },
    '-',
    { titolo: 'Sposta in sezione' },
    ...sezioni.map((s) => ({ testo: s.nome, azione: () => modifica({ sezione_id: s.id }) })),
    { testo: 'Nuova sezione…', icona: 'piu', azione: async () => {
      const nome = await chiediTesto({ titolo: 'Nuova sezione', etichetta: 'Nome della sezione', conferma: 'Crea e sposta' });
      if (!nome) return;
      try { const s = await api.creaSezione(nome, stato.barra.length); await modifica({ sezione_id: s.id }, `${a.nome} in ${nome}`); } catch (e) { avvisa(testoErrore(e)); }
    } },
    a.sezione_id && a.sezione_id !== '__non_assegnati__' ? { testo: 'Rinomina la sezione…', icona: 'matita', azione: () => rinominaSezione(stato.barra.find((s) => s.id === a.sezione_id)) } : null,
    '-',
    { testo: 'Archivia', icona: 'archivia', azione: async () => {
      if (!await chiediConferma({ titolo: `Archiviare ${a.nome}?`, testo: 'Sparisce dalla barra. Le sessioni restano nel database.', conferma: 'Archivia', pericolo: true })) return;
      try { await api.archiviaAssistente(id); await Promise.all([caricaAssistenti(), caricaBarra()]); if (id === stato.assistenteId) azioni.apri(null); } catch (e) { avvisa(testoErrore(e)); }
    } },
  ].filter(Boolean));
}

async function rinominaSezione(s) {
  if (!s || s.id === '__non_assegnati__') return;
  const nome = await chiediTesto({ titolo: 'Rinomina la sezione', etichetta: 'Nome della sezione', valore: s.nome });
  if (!nome || nome === s.nome) return;
  try { await api.modificaSezioni([{ id: s.id, nome }]); await caricaBarra(); avvisa('Sezione rinominata'); } catch (e) { avvisa(testoErrore(e)); }
}

async function nuovaSezione() {
  const nome = await chiediTesto({ titolo: 'Nuova sezione', etichetta: 'Nome della sezione', conferma: 'Crea' });
  if (!nome) return;
  try { await api.creaSezione(nome, stato.barra.length); await caricaBarra(); avvisa(`Sezione «${nome}» creata`); } catch (e) { avvisa(testoErrore(e)); }
}

function menuSezione(ev, s) {
  const speciale = s.id === '__non_assegnati__';
  apriMenu({ x: ev.clientX, y: ev.clientY }, [
    { titolo: s.nome },
    { testo: 'Rinomina…', icona: 'matita', disattivo: speciale, azione: () => rinominaSezione(s) },
    { testo: 'Nuova sezione…', icona: 'piu', azione: () => nuovaSezione() },
  ]);
}
