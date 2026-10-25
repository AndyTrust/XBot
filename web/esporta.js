// Esporta la chat (CONTRATTO-2J lavoro 1): un file Markdown o di testo costruito nel browser dalle
// righe della sessione (GET /sessioni/{id}/righe, a pagine dal cursore 0 finché `altre` è falso).
// Niente input né esiti degli strumenti, niente valori delle richieste: solo nomi ed esiti;
// i token riconoscibili (plc_, Bearer, sk-, ghp_, xox…) escono mascherati come nel registro del server.

import { api, testoErrore } from './api.js';
import { NOME_APP } from './nome.js';
import { assistenteDi, modelliDi, nomeProvider, stato } from './stato.js';
import { campo, foglio } from './ui.js';
import { avvisa, el } from './util.js';

const PAGINA = 500;
const TIPI_RICHIESTA = { domanda: 'domanda', permesso: 'permesso', permesso_locale: 'permesso', cassaforte: 'cassaforte', segreto: 'segreto', modulo: 'modulo', revisione_comando: 'revisione', spostamento_sessione: 'spostamento' };

/** Vero se il browser sa condividere un file (Android, Safari): allora c'è anche «Condividi…». */
export function puoCondividere() {
  try { return !!navigator.canShare?.({ files: [new File(['x'], 'x.txt', { type: 'text/plain' })] }); } catch { return false; }
}

/** Il foglio «Esporta la chat»: Formato e Cosa; `condividi` usa navigator.share invece dello scarico. */
export function apriEsporta({ condividi = false } = {}) {
  const s = stato.sessione;
  if (!s) { avvisa('Nessuna sessione aperta'); return; }
  foglio({
    titolo: condividi ? 'Condividi la chat' : 'Esporta la chat', piccolo: true, unaColonna: true, conferma: condividi ? 'Condividi' : 'Scarica',
    corpo: [
      campo('Formato', { name: 'formato', opzioni: [['md', 'Markdown'], ['txt', 'Testo']], valore: 'md' }, true),
      campo('Cosa', { name: 'cosa', opzioni: [['tutto', 'Questa sessione'], ['messaggi', 'Solo i messaggi, senza strumenti']], valore: 'tutto' }, true),
      el('p.nota.largo', {}, 'Il file nasce qui nel browser. Gli strumenti compaiono solo con nome ed esito; i token restano mascherati.'),
    ],
    invia: async (modulo) => {
      const formato = modulo.querySelector('[name=formato]').value;
      const cosa = modulo.querySelector('[name=cosa]').value;
      let file;
      try { file = await costruisci(s, { formato, strumenti: cosa === 'tutto' }); } catch (e) { throw new Error(`Esportazione non riuscita: ${testoErrore(e)}`); }
      if (condividi) {
        try { await navigator.share({ files: [file], title: file.name }); } catch (e) { if (e?.name !== 'AbortError') throw new Error(`Condivisione non riuscita: ${e?.message || e}`); return true; }
      } else scarica(file);
      avvisa(`${file.name} pronto`);
      return undefined;
    },
  });
}

/** Tutte le righe della sessione, in ordine di `seq`, leggendo a pagine dal cursore 0. */
export async function tutteLeRighe(sessioneId) {
  const viste = new Map();
  let dopo = 0;
  let generazione;
  for (let giro = 0; giro < 1000; giro++) {
    const p = await api.righe(sessioneId, dopo, generazione, PAGINA);
    generazione = p?.generazione;
    const righe = p?.righe || [];
    for (const r of righe) { viste.set(r.id, r); if (r.aggiornata_seq > dopo) dopo = r.aggiornata_seq; }
    if (!p?.altre || !righe.length) break;
  }
  return [...viste.values()].sort((a, b) => a.seq - b.seq);
}

/** Maschera i token come `maschera_testo` del server, più i prefissi comuni dei segreti. */
export function maschera(t) {
  return String(t ?? '')
    .replace(/\b(plc_|sk-|ghp_|gho_|github_pat_|xox[abpr]-)[A-Za-z0-9_\-.~+/=]+/g, '$1…')
    .replace(/\b(Bearer )[A-Za-z0-9_\-.~+/=]+/g, '$1…');
}

const due = (n) => String(n).padStart(2, '0');
const oraDi = (ms) => { const d = new Date(ms); return `${due(d.getHours())}:${due(d.getMinutes())}`; };
const dataDi = (ms) => new Date(ms).toLocaleDateString('it-IT', { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' });
const quando = (ms) => { const d = new Date(ms); return `${due(d.getDate())}/${due(d.getMonth() + 1)}/${d.getFullYear()} ${oraDi(ms)}`; };
const bollo = (d = new Date()) => `${d.getFullYear()}-${due(d.getMonth() + 1)}-${due(d.getDate())}-${due(d.getHours())}${due(d.getMinutes())}`;
const pulito = (t) => String(t || 'assistente').toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, '').replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'assistente';

/** Costruisce il File da scaricare o condividere. */
export async function costruisci(sessione, { formato = 'md', strumenti = true } = {}) {
  const a = assistenteDi(sessione.assistente_id || stato.assistenteId);
  const [righe, nomiAllegati, etichetta] = await Promise.all([
    tutteLeRighe(sessione.id),
    api.libreria(a.id).then((r) => new Map((Array.isArray(r) ? r : r?.file || []).map((f) => [f.id, f.nome]))).catch(() => new Map()),
    sessione.provider_id && sessione.modello_id ? modelliDi(sessione.provider_id).then((m) => m.find((x) => x.id === sessione.modello_id)?.etichetta).catch(() => null) : null,
  ]);
  const testo = componi({ sessione, assistente: a, righe, nomiAllegati, modello: etichetta || sessione.modello_id, formato, strumenti });
  const nome = `${pulito(NOME_APP)}-${pulito(a.nome)}-${bollo()}.${formato === 'txt' ? 'txt' : 'md'}`;
  return new File([testo], nome, { type: formato === 'txt' ? 'text/plain;charset=utf-8' : 'text/markdown;charset=utf-8' });
}

/** Il testo del file: intestazione, poi un blocco per riga con separatori di giorno. */
export function componi({ sessione, assistente, righe, nomiAllegati = new Map(), modello, formato = 'md', strumenti = true }) {
  const md = formato !== 'txt';
  const tu = stato.io?.nome || 'Utente';
  const nomeA = assistente?.nome || 'Assistente';
  const tempi = righe.map((r) => r.creata_ms).filter(Boolean);
  const provider = sessione.provider_id ? nomeProvider(sessione.provider_id) : 'Auto';
  const testa = [
    md ? `# ${nomeA}${sessione.titolo ? ` · ${maschera(sessione.titolo)}` : ''}` : `${nomeA}${sessione.titolo ? ` · ${maschera(sessione.titolo)}` : ''}`,
    '',
    `${md ? '- ' : ''}Assistente: ${nomeA}`,
    `${md ? '- ' : ''}Provider e modello: ${[provider, modello].filter(Boolean).join(' · ')}`,
    `${md ? '- ' : ''}Macchina: ${sessione.macchina_id || 'vps'}`,
    tempi.length ? `${md ? '- ' : ''}Dal ${quando(Math.min(...tempi))} al ${quando(Math.max(...tempi))}` : `${md ? '- ' : ''}Sessione vuota`,
    `${md ? '- ' : ''}Esportata il ${quando(Date.now())} da ${NOME_APP}${strumenti ? '' : ', solo i messaggi'}`,
    '',
  ];
  const corpo = [];
  let giorno = '';
  const citazione = (t) => (md ? `> ${t}` : `  [${t}]`);
  for (const r of righe) {
    const c = r.corpo || {};
    const blocco = rigaDiTesto(r, c, { tu, nomeA, md, strumenti, nomiAllegati, citazione });
    if (!blocco) continue;
    const g = dataDi(r.creata_ms);
    if (g !== giorno) { giorno = g; corpo.push(md ? `## ${g}` : `--- ${g} ---`, ''); }
    corpo.push(blocco, '');
  }
  return `${[...testa, ...corpo].join('\n').replace(/\n{3,}/g, '\n\n').trim()}\n`;
}

function rigaDiTesto(r, c, { tu, nomeA, md, strumenti, nomiAllegati, citazione }) {
  const ora = oraDi(r.creata_ms);
  const firma = (chi) => (md ? `**${chi}** (${ora})` : `${chi} (${ora})`);
  if (r.genere === 'messaggio') {
    let chi = r.ruolo === 'utente' ? tu : nomeA;
    if (r.ruolo === 'collega') chi = `${c.mittente_nome || c.mittente || 'Collega'} → ${nomeA}`;
    if (r.ruolo === 'utente' && /^routine:/.test(c.origine || '')) chi = `${tu} (routine)`;
    const allegati = (c.allegati || []).map((x) => (typeof x === 'string' ? nomiAllegati.get(x) : x?.nome) || 'allegato');
    const righe = [firma(chi), maschera(c.testo || '').trim()];
    if (allegati.length) righe.push(md ? `_Allegati: ${allegati.map(maschera).join(', ')}_` : `Allegati: ${allegati.map(maschera).join(', ')}`);
    return righe.filter(Boolean).join('\n');
  }
  if (!strumenti) return null;
  if (r.genere === 'chiamata_strumento' || r.genere === 'esito') {
    const esito = c.stato === 'in_corso' ? 'in corso' : (c.ok === true || c.stato === 'riuscito') ? 'ok' : 'fallito';
    return citazione(`strumento: ${maschera(c.nome || 'strumento')} (${esito}, ${ora})`);
  }
  if (r.genere === 'richiesta') {
    const tipo = TIPI_RICHIESTA[c.tipo] || 'richiesta';
    return citazione(`richiesta di ${tipo}: ${c.stato || 'aperta'} (${ora})`);
  }
  if (r.genere === 'avviso' || r.genere === 'evento') {
    if (c.tipo === 'chiamata_vocale') return citazione(`chat vocale di ${Math.round((c.durata_ms || 0) / 1000)} s (${ora})`);
    return citazione(`${r.genere}: ${maschera(c.testo || c.codice || '')} (${ora})`);
  }
  return null; // ragionamento e altri generi restano fuori dal file
}

function scarica(file) {
  const url = URL.createObjectURL(file);
  const a = el('a', { href: url, download: file.name, hidden: true });
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 30000);
}
