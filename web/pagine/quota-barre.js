// Le barre delle finestre di quota (CONTRATTO-2J lavoro 4): sotto l'anello di un provider con
// `quota_fonte = 'client'`, una barra per finestra presente in `dettaglio_json`
// (`five_hour`, `seven_day`: {percentuale_usata, azzera_ms}); rossa sopra il 90 %. Niente barra se il dato manca.

import { quotaDi } from '../stato.js';
import { el, numero } from '../util.js';

const FINESTRE = [['five_hour', '5 ore'], ['seven_day', '7 giorni']];

/** «fra 12 min», «fra 1 h 20», «fra 4 giorni». */
export function fraTempo(ms) {
  const min = Math.round((ms - Date.now()) / 60000);
  if (!Number.isFinite(min)) return '';
  if (min <= 0) return 'adesso';
  if (min < 60) return `fra ${min} min`;
  if (min < 48 * 60) { const m = min % 60; return `fra ${Math.floor(min / 60)} h${m ? ` ${String(m).padStart(2, '0')}` : ''}`; }
  const g = Math.round(min / 1440);
  return `fra ${g} ${g === 1 ? 'giorno' : 'giorni'}`;
}

/** «HH:MM» (o «ieri HH:MM», «12/10 HH:MM») di una lettura. */
function oraLettura(ms) {
  const d = new Date(ms);
  const ora = d.toLocaleTimeString('it-IT', { hour: '2-digit', minute: '2-digit' });
  const oggi = new Date();
  if (d.toDateString() === oggi.toDateString()) return ora;
  const ieri = new Date(oggi.getTime() - 86400000);
  return d.toDateString() === ieri.toDateString() ? `ieri ${ora}` : `${d.toLocaleDateString('it-IT', { day: '2-digit', month: '2-digit' })} ${ora}`;
}

/** 2.L difetto 6: la rilevazione è vecchia se il server la segna `scaduta` o se `azzera_ms` è passato. */
export const quotaScaduta = (q, adesso = Date.now()) => !!q && (q.scaduta === true || (Number(q.azzera_ms) > 0 && Number(q.azzera_ms) < adesso));

/** «lettura delle 06:41, finestra già azzerata» (senza ora se il server non la dice). */
export const fraseScaduta = (q) => (q?.rilevata_ms ? `lettura delle ${oraLettura(q.rilevata_ms)}, finestra già azzerata` : 'lettura vecchia, finestra già azzerata');

export function barreQuota(providerId, fonte) {
  if (fonte !== 'client') return null;
  const q = quotaDi(providerId);
  const d = q?.dettaglio;
  if (!d || typeof d !== 'object') return null;
  const barre = FINESTRE.map(([chiave, nome]) => {
    const v = d[chiave]?.percentuale_usata;
    if (v === null || v === undefined || v === '' || !Number.isFinite(Number(v))) return null;
    const u = Math.max(0, Math.min(100, Number(v)));
    const azzera = Number(d[chiave]?.azzera_ms);
    // finestra già azzerata: la riga dice da quando è la lettura, in grigio, e la barra non si disegna
    if (quotaScaduta({ scaduta: q.scaduta === true && !(azzera > Date.now()), azzera_ms: azzera })) {
      return el('div.barra-quota.scaduta', { dati: { finestra: chiave } }, el('span.barra-quota-testo', {}, `${nome} · ${fraseScaduta(q)}`));
    }
    const testo = [nome, `${numero(u)} %`, azzera > 0 ? `si azzera ${fraTempo(azzera)}` : null].filter(Boolean).join(' · ');
    return el(`div.barra-quota${u > 90 ? '.alta' : ''}`, { dati: { finestra: chiave } },
      el('span.barra-quota-testo', {}, testo),
      el('span.barra-quota-binario', { role: 'progressbar', 'aria-label': `${nome}: ${numero(u)} % usata`, 'aria-valuemin': '0', 'aria-valuemax': '100', 'aria-valuenow': String(Math.round(u)) },
        el('span.barra-quota-pieno', { style: `width:${u}%` })));
  }).filter(Boolean);
  return barre.length ? el('div.barre-quota', {}, barre) : null;
}
