// Le sezioni del pannello che leggono dal server: MEMORIA, FILE, SEGRETI (Dettagli), la Libreria
// e la scheda Computer (macchina della sessione, battito, ultimi lavori e turni, «Continua su…»).

import { providerDi } from './macchine.js';
import { api, elenco, testoErrore } from './api.js';
import { icona } from './icone.js';
import { bus } from './stato.js';
import { tastino, tastoIcona, titolo, vuoto } from './ui.js';
import { dimensione, durata, el, quandoBreve } from './util.js';

const caricando = () => el('p.nota', {}, 'Carico…');

/** «…/Memoria/00 Comune/Memoria.md» → «00 Comune/Memoria.md»: le ultime due parti, il resto nel titolo. */
const breve = (p) => { const s = String(p || '').split('/').filter(Boolean); return s.length > 2 ? s.slice(-2).join('/') : String(p || ''); };
const percorso = (p) => el('span', { title: p }, breve(p));

/** Riempie un riquadro con il risultato di `f` (errore in chiaro). */
async function riempiCon(box, f) {
  box.replaceChildren(caricando());
  try { box.replaceChildren(...[await f()].flat().filter(Boolean)); } catch (e) { box.replaceChildren(vuoto(testoErrore(e))); }
}

export function sezioneMemoria(aid) {
  const box = el('div.riquadro.elenco');
  riempiCon(box, async () => {
    const m = await api.memoria(aid);
    const note = elenco(m?.note, 'note').slice(0, 4);
    const scritture = elenco(m?.scritture, 'scritture').slice(0, 4);
    return [
      el('div.riga-elenco', {}, el('span.riga-testi', {}, el('strong', {}, 'Cartella'), m?.cartella ? el('small', { title: m.cartella }, breve(m.cartella)) : el('small', {}, 'nessuna: si imposta dalle impostazioni dell\'assistente'))),
      ...note.map((n) => el('div.riga-elenco', {}, icona('file', 14), el('span.riga-testi', {}, percorso(n.percorso), el('small', {}, n.aggiornata_ms ? quandoBreve(n.aggiornata_ms) : '')))),
      scritture.length ? el('div.riga-elenco.sotto-titolo', {}, el('small', {}, 'Ultime scritture')) : null,
      ...scritture.map((w) => el('div.riga-elenco', {}, icona('matita', 14), el('span.riga-testi', {}, percorso(w.percorso), el('small', {}, `${w.tipo || ''} · ${w.ms ? quandoBreve(w.ms) : ''}`)))),
    ];
  });
  return el('div.sezione-pannello', {}, titolo('Memoria'), box);
}

function rigaFile(f, grande = false) {
  const url = api.urlBlob(f.blob_hash);
  const immagine = /^image\//.test(f.tipo_mime || '');
  return el(`div.riga-elenco.file${grande ? '.grande' : ''}`, {},
    immagine ? el('a.miniatura', { href: url, target: '_blank', rel: 'noopener', 'aria-label': `Apri ${f.nome}` }, el('img', { src: url, alt: '', loading: 'lazy' })) : el('span.miniatura', {}, icona('file', grande ? 22 : 16)),
    el('span.riga-testi', {}, el('span', {}, f.nome), el('small', {}, [dimensione(f.dimensione), f.caricato_ms ? quandoBreve(f.caricato_ms) : ''].filter(Boolean).join(' · '))),
    el('a.tasto-icona.piccolo', { href: url, download: f.nome || true, 'aria-label': `Scarica ${f.nome}`, title: 'Scarica' }, icona('scarica', 15)));
}

export function sezioneFile(aid, apriLibreria) {
  const box = el('div.riquadro.elenco');
  riempiCon(box, async () => {
    const file = elenco(await api.libreria(aid), 'file');
    if (!file.length) return vuoto('Nessun file: gli allegati e i documenti prodotti finiscono qui.');
    return [...file.slice(0, 3).map((f) => rigaFile(f)), file.length > 3 ? el('button.tasto.piccolo.largo', { type: 'button', onclick: apriLibreria }, `Tutti i ${file.length} file`) : null];
  });
  return el('div.sezione-pannello', {}, titolo('File', tastino('Libreria', 'immagine', apriLibreria)), box);
}

/** La scheda Libreria: tutti i file dell'assistente, con anteprima delle immagini e scarico. */
export function sezioneLibreria(aid) {
  const griglia = el('div.libreria');
  const box = el('div.riquadro.elenco');
  riempiCon(box, async () => {
    const file = elenco(await api.libreria(aid), 'file');
    const immagini = file.filter((f) => /^image\//.test(f.tipo_mime || ''));
    griglia.replaceChildren(...immagini.slice(0, 12).map((f) => el('a', { href: api.urlBlob(f.blob_hash), target: '_blank', rel: 'noopener', title: f.nome, 'aria-label': `Apri ${f.nome}` },
      el('img', { src: api.urlBlob(f.blob_hash), alt: f.nome, loading: 'lazy' }))));
    return file.length ? file.map((f) => rigaFile(f, true)) : vuoto('La Libreria è vuota: gli allegati delle sessioni e i documenti prodotti arrivano qui.');
  });
  return el('div.sezione-pannello', {}, titolo('Immagini'), griglia, titolo('Tutti i file'), box);
}

export function sezioneSegreti(aid, apriCassaforte) {
  const box = el('div.riquadro.elenco');
  riempiCon(box, async () => {
    const voci = elenco(await api.cassaforte('segreto'), 'voci');
    const suoi = voci.filter((v) => !Array.isArray(v.assistenti) || v.assistenti.includes(aid));
    if (!suoi.length) return vuoto('Nessun segreto per questo assistente.');
    return suoi.map((v) => el('div.riga-elenco', {}, icona('chiave', 14),
      el('span.riga-testi', {}, el('code', {}, v.riferimento?.variabile || v.titolo), el('small', {}, v.riferimento?.descrizione || ''))));
  });
  return el('div.sezione-pannello', {}, titolo('Segreti', tastoIcona('piu', 'Nuovo segreto in cassaforte', () => apriCassaforte?.({ nuovo: 'segreto', assistenti: [aid] }))), box);
}

const STATI = { connessa: 'connessa', avvio: 'si avvia', riconnessione: 'si riconnette', ferma: 'ferma', rimossa: 'rimossa' };

/** La scheda Computer. `sessione()` dà la sessione aperta; `sposta(id)` la continua su un'altra macchina. */
export function sezioneComputer(sessione, sposta) {
  const testa = el('div.riquadro');
  const lavori = el('div.riquadro.elenco');
  const nodo = el('div.sezione-pannello', {}, titolo('Macchina della sessione'), testa, titolo('Ultimi lavori e turni'), lavori);
  const carica = async () => {
    testa.replaceChildren(caricando());
    let macchine = [];
    try { macchine = elenco(await api.macchine(), 'macchine'); } catch (e) { testa.replaceChildren(vuoto(testoErrore(e))); return; }
    const s = sessione();
    const m = macchine.find((x) => x.id === s?.macchina_id) || macchine.find((x) => ['vps', 'mac'].includes(x.genere) && x.stato === 'connessa') || macchine[0];
    if (!m) { testa.replaceChildren(vuoto('Nessuna macchina registrata.')); lavori.replaceChildren(); return; }
    const altre = macchine.filter((x) => x.id !== m.id && x.stato === 'connessa');
    const sceltaAltra = el('select', { 'aria-label': 'Continua su un\'altra macchina' }, el('option', { value: '' }, 'Continua su…'), altre.map((x) => el('option', { value: x.id }, x.nome)));
    sceltaAltra.onchange = () => { if (sceltaAltra.value) sposta(sceltaAltra.value); };
    testa.replaceChildren(
      el('div.coppia', {}, el('span', {}, 'Nome'), el('strong', {}, `${m.nome} · ${m.genere}`)),
      el('div.coppia', {}, el('span', {}, 'Stato'), el('span', {}, el(`span.punto-${m.stato === 'connessa' ? 'verde' : 'grigio'}`), STATI[m.stato] || m.stato)),
      el('div.coppia', {}, el('span', {}, 'Battito'), el('span', {}, m.ultimo_battito_ms ? quandoBreve(m.ultimo_battito_ms) : 'mai')),
      el('div.coppia', {}, el('span', {}, 'Posti'), el('span', {}, m.posti_liberi !== undefined && m.posti_liberi !== null ? `${m.posti_liberi} liberi su ${m.posti ?? 1}` : String(m.posti ?? 1))),
      el('div.coppia', {}, el('span', {}, 'Provider'), el('span', {}, providerDi(m).map((p) => p.nome).join(' · ') || '—')),
      el('div.coppia', {}, el('span', {}, 'Continua su'), altre.length ? sceltaAltra : el('span.nota', {}, 'nessun\'altra macchina connessa')),
    );
    await riempiCon(lavori, async () => {
      const r = await api.lavori(m.id, 12);
      const turni = elenco(r?.turni, 'turni').map((t) => ({ ...t, tipo: 'turno', descrizione: `Turno${t.assistente ? ` di ${t.assistente}` : ''}${t.modello_id ? ` · ${t.modello_id}` : ''}`,
        esito: t.errore_codice || t.stato, creato_ms: t.creato_ms || t.avviato_ms || t.iniziato_ms }));
      const l = [...elenco(r, 'lavori'), ...turni].sort((a, b) => (b.creato_ms || 0) - (a.creato_ms || 0)).slice(0, 15);
      if (!l.length) return vuoto('Nessun lavoro registrato su questa macchina.');
      return l.map((x) => {
        const ok = ['riuscito', 'ok'].includes(x.stato) || x.esito === 'riuscito';
        const durataMs = x.durata_ms ?? (x.finito_ms && (x.preso_ms || x.creato_ms) ? x.finito_ms - (x.preso_ms || x.creato_ms) : null);
        return el('div.riga-elenco', {},
          el(`span.esito.${ok ? 'ok' : ['in_corso', 'in_coda', 'preso'].includes(x.stato) ? 'corso' : 'no'}`, { 'aria-label': x.stato }, ok ? '✓' : ['in_corso', 'in_coda', 'preso'].includes(x.stato) ? '◌' : '✗'),
          el('span.riga-testi', {}, el('span', {}, x.descrizione || x.capacita || x.tipo || 'lavoro'),
            el('small', {}, [x.tipo, quandoBreve(x.creato_ms || x.ms || Date.now()), durata(durataMs), typeof x.esito === 'string' ? x.esito : x.stato].filter(Boolean).join(' · '))));
      });
    });
  };
  carica();
  // 2.L difetto 1: dopo il risveglio dello schermo lo stato si rilegge (niente errore di rete vecchio)
  const togli = bus.su('pagina:visibile', () => { if (nodo.isConnected) carica(); else togli(); });
  return nodo;
}
