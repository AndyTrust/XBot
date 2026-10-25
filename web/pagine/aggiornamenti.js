// Aggiornamenti (Layout sez. 2.8): aggiornamento dell'installazione (CONTRATTO-GITHUB sez. E: versione
// installata e nuova, note, modo, «Aggiorna ora»), versione del server e della app, «Controlla», eventi
// di sistema con una frase italiana per tipo (2.L difetto 8).

import { agg, aggiornaOra, controlla as controllaServer } from '../aggiorna.js';
import { api, elenco } from '../api.js';
import { etichettaMacchina, leggiMacchine, macchinaDa } from '../macchine.js';
import { NOME_APP } from '../nome.js';
import { nomeProvider } from '../stato.js';
import { coppia, vuoto } from '../ui.js';
import { avvisa, el, quandoBreve } from '../util.js';

async function versioneApp() {
  try {
    const chiavi = await caches.keys();
    return chiavi.find((k) => k.startsWith('xbot-web-')) || 'senza cache';
  } catch { return 'senza cache'; }
}

// 2.L difetto 8: una frase italiana per ogni tipo di evento (i tipi di `eventi::scrivi` del server).
const MOTIVI = { socket_chiuso: 'il collegamento si è chiuso', lease: 'non ha risposto in tempo a un lavoro' };
const macchina = (id) => { const m = id ? macchinaDa(id) : null; return m ? etichettaMacchina(m) : (id || 'una macchina'); };
const collegamento = (d) => (d.nome ? `«${d.nome}»` : 'un collegamento');
const conAccount = (d) => (d.account ? ` (${d.account})` : '');
const FRASI = {
  avvio: (d) => `Server avviato${d.versione ? `, versione ${d.versione}` : ''}`,
  provider_attivo: (d) => `${nomeProvider(d.provider_id)} ${d.attivo ? 'acceso' : 'spento'}`,
  macchina_collegata: (d) => `${macchina(d.macchina_id)} collegato`,
  macchina_persa: (d) => `${macchina(d.macchina_id)} scollegato${MOTIVI[d.motivo] ? `: ${MOTIVI[d.motivo]}` : ''}`,
  macchina_registrata: (d) => `Computer nuovo registrato: ${d.nome || macchina(d.macchina_id)}`,
  macchina_rimossa: (d) => `${macchina(d.macchina_id)} rimosso`,
  oauth_collegato: (d) => `${collegamento(d)}: account collegato${conAccount(d)}`,
  oauth_scaduto: (d) => `${collegamento(d)}: accesso scaduto${conAccount(d)}, va ricollegato`,
  oauth_scollegato: (d) => `${collegamento(d)}: account scollegato${conAccount(d)}`,
  lavoro_in_attesa: (d) => `Un lavoro aspetta ${macchina(d.macchina_id)}, che è spento`,
};

/** La frase di un evento; i tipi senza frase dicono solo «evento <tipo>», mai il dettaglio tecnico. */
export function testoEvento(e) {
  let d = e.dettaglio_json ?? e.dettaglio ?? {};
  if (typeof d === 'string') { try { d = JSON.parse(d); } catch { d = {}; } }
  const f = FRASI[e.tipo];
  return f ? f(d && typeof d === 'object' ? d : {}) : `evento ${e.tipo || 'senza tipo'}`;
}

const MODI = {
  avvisa: 'Avvisa: compare un avviso e si aggiorna col tasto',
  auto: 'Automatico: ogni notte alle 04:00',
  no: 'Spento: nessun avviso, nessun aggiornamento',
};
const breve = (v) => (v ? String(v).split(/\s+/)[0] : '—');

/** Il riquadro dell'aggiornamento dell'installazione (GET /aggiornamenti). */
function riquadroAggiornamento(ridisegna) {
  const d = agg.dati;
  const e = agg.errore;
  const righe = [];
  if (!d) {
    righe.push(coppia('Stato', el('span', {}, e?.stato === 404 ? 'Questo server non controlla gli aggiornamenti.'
      : e ? 'Controllo non riuscito: il server non risponde.' : 'Controllo in corso…')));
  } else {
    const stato = d.disponibile === true ? `Disponibile ${breve(d.versione_remota)}`
      : d.disponibile === false ? 'Già all\'ultima versione' : `Non verificato${d.errore ? `: ${d.errore}` : ''}`;
    righe.push(
      coppia('Installata', el('strong', {}, breve(d.versione_installata || agg.installata)), d.versione_installata?.includes(' ') ? d.versione_installata.split(/\s+/).slice(1).join(' ') : null),
      coppia('Ultima pubblicata', el('span', {}, d.versione_remota ? breve(d.versione_remota) : '—')),
      coppia('Stato', el('span', {}, stato)),
      d.note_url ? coppia('Novità', el('a', { href: d.note_url, target: '_blank', rel: 'noopener' }, 'Leggi le note della versione')) : null,
      coppia('Controllato', el('span', {}, d.controllato_ms ? quandoBreve(d.controllato_ms) : '—')),
      coppia('Modo', el('span', {}, MODI[d.modo] || d.modo || '—'), 'Si cambia sul server: installa.sh --aggiornamento auto | avvisa | no'));
  }
  if (agg.installataNuova) righe.push(coppia('Installata ora', el('button.tasto.piccolo', { type: 'button', onclick: () => location.reload() }, `Ricarica (${breve(agg.installataNuova)})`)));
  const puo = d?.disponibile === true && d.modo !== 'no' && !agg.installataNuova;
  righe.push(el('div.riga-tasti', {},
    puo ? el('button.tasto', { type: 'button', disabled: agg.inCorso, onclick: () => aggiornaOra(ridisegna) }, agg.inCorso ? 'Aggiornamento in corso…' : 'Aggiorna ora') : null,
    el('button.tasto.piccolo', { type: 'button', onclick: async () => { await controllaServer(); ridisegna(); } }, 'Controlla adesso')));
  return [el('div.riquadro-titolo', {}, 'Aggiornamento'), el('div.riquadro', {}, righe)];
}

/** Il commit si mostra solo se c'è davvero (senza git il server scrive «sconosciuto»). */
const commitVero = (c) => (c && !/^sconosciut/i.test(String(c)) ? ` · ${String(c).slice(0, 8)}` : '');

export async function paginaAggiornamenti({ ridisegna }) {
  if (!agg.dati && !agg.errore) await controllaServer();
  const [v, ev, app] = await Promise.all([api.versione(), api.eventi(50).catch(() => ({ eventi: [] })), versioneApp(), leggiMacchine()]);
  const eventi = elenco(ev, 'eventi');
  const controlla = async () => {
    try {
      const reg = await navigator.serviceWorker?.getRegistration('/app/');
      await reg?.update();
      avvisa(reg?.waiting || reg?.installing ? 'Nuova versione scaricata: si applica al prossimo avvio' : 'Già aggiornata');
    } catch { avvisa('Controllo non riuscito'); }
    ridisegna();
  };
  return [
    ...riquadroAggiornamento(ridisegna),
    el('div.riquadro-titolo', {}, 'Versione'),
    el('div.riquadro', {},
      coppia(`Server di ${NOME_APP}`, el('strong', {}, `${v?.versione || '—'}${commitVero(v?.commit)}`)),
      coppia('Compilato', el('span', {}, v?.compilato_ms ? quandoBreve(v.compilato_ms) : '—')),
      coppia('Avviato', el('span', {}, v?.avviato_ms ? quandoBreve(v.avviato_ms) : '—')),
      coppia('Schema dei dati', el('span', {}, String(v?.schema ?? '—'))),
      coppia('Macchina', el('span', {}, typeof v?.macchina === 'object' && v.macchina ? `${v.macchina.nome || v.macchina.id} · ${v.macchina.genere || ''}` : v?.macchina || '—')),
      coppia('App (cache)', el('span', {}, app)),
      coppia('Controlla', el('button.tasto.piccolo', { type: 'button', onclick: controlla }, 'Controlla aggiornamenti'))),
    el('div.riquadro-titolo', {}, 'Eventi di sistema'),
    el('div.riquadro.elenco', {}, eventi.length ? eventi.map((e) => el('div.riga-elenco', {},
      el('span.riga-testi', {}, el('span', {}, testoEvento(e)),
        el('small', {}, quandoBreve(e.ms || e.creato_ms || e.creata_ms || Date.now()))))) : vuoto('Nessun evento registrato.')),
  ];
}
