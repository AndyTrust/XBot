// Macchine e scelte per macchina (CONTRATTO-2E sez. 3 e 4): elenco condiviso delle macchine,
// provider e modelli di ogni macchina, scelta «macchina · provider · modello» raggruppata.
// La VPS (macchina del server) c'è sempre; le altre solo se `connesse`.

import { api, elenco } from './api.js';
import { bus, modelliDi, nomeProvider, providerAcceso, providerAuto, stato } from './stato.js';

export let macchine = [];

export const STATI_MACCHINA = { connessa: 'Online', avvio: 'Si avvia', riconnessione: 'Si riconnette', ferma: 'Offline', rimossa: 'Rimossa' };

export async function leggiMacchine() {
  try { macchine = elenco(await api.macchine(), 'macchine').filter((m) => m.stato !== 'rimossa'); } catch { macchine = []; }
  bus.emetti('macchine');
  return macchine;
}

/** In linea: `in_linea` del server (canale aperto), altrimenti lo stato `connessa`. */
export const inLinea = (m) => !!m && (m.in_linea !== undefined && m.in_linea !== null ? !!m.in_linea : m.stato === 'connessa');

/** La macchina del server: `e_il_server` (S-B), altrimenti genere `vps`. */
export const macchinaServer = () => macchine.find((m) => m.e_il_server || m.del_server) || macchine.find((m) => m.genere === 'vps') || null;
export const macchinaMac = () => macchine.find((m) => m.genere === 'mac') || null;
export const macchinaDa = (id) => macchine.find((m) => m.id === id) || null;

/** Nome breve per le scelte: «VPS», «Mac», altrimenti il nome registrato. */
export function etichettaMacchina(m) {
  if (!m) return 'VPS';
  const doppi = macchine.filter((x) => x.genere === m.genere).length > 1;
  if (m.genere === 'vps') return doppi ? `VPS ${m.nome}` : 'VPS';
  if (m.genere === 'mac') {
    if (!m.nome || (/^mac$/i.test(m.nome) && !doppi)) return 'Mac';
    return /mac/i.test(m.nome) ? m.nome : `Mac ${m.nome}`;
  }
  return m.nome || m.id;
}

const idDi = (x) => (typeof x === 'string' ? x : x?.macchina_id || x?.id);

/** I provider di una macchina: dal campo `provider[]` della macchina, altrimenti da `macchine[]` del provider. */
export function providerDi(m) {
  if (!m) return [];
  if (!m.id && !macchine.length) return stato.provider.filter((p) => p.attivo && p.caricato !== false); // macchine non lette
  const propri = Array.isArray(m.provider) ? m.provider : [];
  if (propri.length) {
    return propri.map((p) => (typeof p === 'string' ? { id: p } : p)).map((p) => {
      const glob = stato.provider.find((x) => x.id === p.id) || {};
      return { ...glob, ...p, nome: p.nome || glob.nome || p.id };
    }).filter((p) => p.attivo !== 0 && p.attivo !== false && providerAcceso(p.id));
  }
  const server = macchinaServer();
  return stato.provider.filter((p) => p.attivo && p.caricato !== false).filter((p) => {
    const dove = Array.isArray(p.macchine) ? p.macchine.map(idDi) : null;
    if (dove && dove.length) return dove.includes(m.id);
    return m === server || (!server && m.genere === 'vps') || p.macchina_id === m.id; // prima della 2.E: tutto sul server
  });
}

/** Le macchine in cui si può scegliere: la VPS e le altre connesse con almeno un provider. */
export function macchineDiScelta({ tutte = false } = {}) {
  const server = macchinaServer();
  const altre = macchine.filter((m) => m !== server && (tutte || inLinea(m)) && providerDi(m).length);
  return [server || { id: '', nome: 'server', genere: 'vps', stato: 'connessa' }, ...altre];
}

/** I livelli di pensiero di un modello: `livelli` (dalla macchina) o `livelli_ragionamento_json` (da /modelli). */
export function livelliDi(x) {
  let l = x?.livelli ?? x?.livelli_ragionamento_json ?? [];
  if (typeof l === 'string') { try { l = JSON.parse(l); } catch { l = []; } }
  return Array.isArray(l) ? l.filter(Boolean).map(String) : [];
}

async function modelliIn(p) {
  if (Array.isArray(p.modelli) && p.modelli.length) return p.modelli.map((x) => (typeof x === 'string' ? { id: x, etichetta: x, disponibile: 1 } : x));
  try { return await modelliDi(p.id); } catch { return []; }
}

export const valoreScelta = (macchinaId, providerId, modelloId) => `${macchinaId || ''}|${providerId || ''}|${modelloId || ''}`;
export function leggiScelta(v) { const [macchina_id, provider_id, modello_id] = String(v).split('|'); return { macchina_id: macchina_id || null, provider_id: provider_id || null, modello_id: modello_id || null }; }

/**
 * I gruppi della scelta: [{macchina, voci:[{valore, testo, disattivo, macchina, provider, modello, etichetta, livelli}]}].
 * La prima voce del server è «Auto»; ogni provider con modelli ha anche «predefinito».
 * Il testo intero è «Mac · Cursor · Grok 4.7». Nessun filtro: ogni modello dichiarato entra
 * (anche i «-fast»). `tutte`: anche le macchine spente (foglio dell'assistente).
 */
export async function gruppiScelta({ tutte: ancheSpente = false } = {}) {
  const auto = providerAuto();
  const gruppi = [];
  const tutte = macchineDiScelta({ tutte: ancheSpente });
  for (const m of tutte) {
    const em = etichettaMacchina(m);
    const voci = [];
    const viste = new Set();
    const metti = (v) => { if (!viste.has(v.valore)) { viste.add(v.valore); voci.push(v); } };
    if (m === tutte[0]) metti({ valore: valoreScelta(m.id, '', ''), testo: `${em} · Auto${auto ? ` (${nomeProvider(auto)})` : ''}`, macchina: m, provider: null, modello: null, etichetta: `Auto${auto ? ` (${nomeProvider(auto)})` : ''}`, livelli: [] });
    for (const p of providerDi(m)) {
      const np = p.nome || nomeProvider(p.id) || p.id;
      const ms = (await modelliIn(p)).filter((x) => x && etichettaModello(x, p));
      metti({ valore: valoreScelta(m.id, p.id, ''), testo: ms.length ? `${em} · ${np} · predefinito` : `${em} · ${np}`, disattivo: p.caricato === false,
        macchina: m, provider: p, modello: null, etichetta: 'predefinito del provider', livelli: ms.length ? livelliDi(ms[0]) : [] });
      for (const x of ms) {
        const et = etichettaModello(x, p);
        metti({ valore: valoreScelta(m.id, p.id, x.id), testo: `${em} · ${np} · ${et}`, disattivo: !x.disponibile && x.disponibile !== undefined,
          macchina: m, provider: p, modello: x, etichetta: et, livelli: livelliDi(x) });
      }
    }
    if (voci.length) gruppi.push({ macchina: m, voci }); // niente gruppi vuoti nella tendina
  }
  return gruppi;
}

/**
 * L'etichetta leggibile di un modello: spazi in eccesso tolti e, quando il nome del provider
 * è già scritto accanto, senza la coda «(Cursor)» che il client aggiunge ai Grok.
 * Vuota se il modello non ha né etichetta né id (la voce si salta).
 */
export function etichettaModello(x, p) {
  const t = String((typeof x === 'string' ? x : x.etichetta || x.id) || '').replace(/[\u200b\s]+/g, ' ').trim();
  const nome = String(p?.nome || '').split(/[\s/]+/)[0];
  return nome && t.toLowerCase().endsWith(` (${nome.toLowerCase()})`) ? t.slice(0, -(nome.length + 3)).trim() : t;
}
