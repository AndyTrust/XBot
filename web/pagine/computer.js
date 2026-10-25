// Computer (CONTRATTO-2E sez. 4): macchine vere con stato, battito, posti, capacità, provider e
// conteggio dei modelli per provider con «mostra tutti» (foglio di scelta); permesso di esecuzione; cartelle concesse; rimuovi; «Collega un computer»
// con lo scarico del lavoratore e il comando `xbot-worker enroll <origine> <biglietto>`.

import { api, elenco, testoErrore } from '../api.js';
import { icona } from '../icone.js';
import { etichettaModello, inLinea, leggiMacchine } from '../macchine.js';
import { chiediConferma } from '../menu.js';
import { apriPcWindows, infoPcWindows } from '../pc-windows.js';
import { apriSceltaModello } from '../scelta.js';
import { modelliDi, providerAcceso } from '../stato.js';
import { daCopiare, foglio, nota, tendina, vuoto } from '../ui.js';
import { avvisa, el, fraQuanto, quandoBreve } from '../util.js';

const PERMESSI = [['always', 'Consenti sempre'], ['ask', 'Chiedi ogni volta'], ['never', 'Mai']];
const ICONE = { vps: 'computer', mac: 'computer', android: 'telefono', ios: 'telefono', windows: 'computer' };
const STATI = { connessa: 'Online', avvio: 'Si avvia', riconnessione: 'Si riconnette', ferma: 'Offline', rimossa: 'Rimossa' };
const CAPACITA = { turni: 'turni', chrome: 'Chrome', file: 'file', exec: 'comandi', schermo: 'schermo' };
const SCARICA = '/api/v1/scarica/xbot-worker-macos'; // S-B: sotto /api/v1, con l'accesso

const delServer = (m) => m.e_il_server || m.del_server || (m.e_il_server === undefined && m.genere === 'vps');

/** Il comando da copiare: quello del server se c'è, altrimenti con l'origine di questa pagina. */
const comandoDa = (b) => (typeof b.comando === 'string' && b.comando.startsWith('xbot-worker enroll ') ? b.comando : `xbot-worker enroll ${location.origin} ${b.biglietto}`);

async function collega(ridisegna) {
  let b;
  try { b = await api.biglietto(); } catch (e) { avvisa(testoErrore(e)); return; }
  const prima = new Set((await leggiMacchine()).map((m) => m.id));
  const attesa = el('p.nota', { role: 'status' }, 'Aspetto il nuovo computer…');
  const passo = (n, titolo, ...dentro) => el('div.passo', {}, el('span.passo-numero', {}, String(n)), el('div.passo-testo', {}, el('strong', {}, titolo), ...dentro));
  const { nodo } = foglio({
    titolo: 'Collega un computer', unaColonna: true,
    corpo: [
      passo(1, 'Scarica il lavoratore',
        el('a.tasto.piccolo', { href: SCARICA, download: 'xbot-worker' }, icona('scarica', 14), 'xbot-worker per Mac'),
        el('small', {}, 'Poi, nel Terminale della cartella dove l\'hai scaricato:'),
        daCopiare('chmod +x xbot-worker && xattr -d com.apple.quarantine xbot-worker 2>/dev/null; sudo mv xbot-worker /usr/local/bin/')),
      passo(2, 'Collegalo con il biglietto', daCopiare(comandoDa(b)),
        el('small', {}, b.scade_ms ? `Il biglietto vale una volta sola e scade ${fraQuanto(b.scade_ms)}.` : 'Il biglietto vale una volta sola.')),
      passo(3, 'Tienilo acceso', daCopiare('xbot-worker install-launchagent'),
        el('small', {}, 'Parte a ogni accesso e si ricollega da solo. Trova Claude Code, Cursor e Grok già autenticati.')),
      attesa,
    ],
  });
  // finché il foglio è aperto guardo ogni 4 s se il computer nuovo è arrivato
  const giro = setInterval(async () => {
    if (!nodo.isConnected) { clearInterval(giro); ridisegna(); return; }
    if (document.hidden) return; // 2.L difetto 11
    const nuove = (await leggiMacchine()).filter((m) => !prima.has(m.id));
    if (nuove.length) {
      clearInterval(giro);
      attesa.textContent = `${nuove[0].nome} è collegato.`;
      avvisa(`${nuove[0].nome} collegato`);
      ridisegna();
    }
  }, 4000);
}

async function rimuovi(m, ridisegna) {
  const ok = await chiediConferma({
    titolo: `Rimuovere ${m.nome}?`, conferma: 'Rimuovi', pericolo: true,
    testo: 'La credenziale del lavoratore viene revocata: per riaverlo serve un biglietto nuovo. Le sessioni su questa macchina tornano sulla VPS.',
  });
  if (!ok) return;
  try { await api.togliMacchina(m.id); avvisa(`${m.nome} rimosso`); await leggiMacchine(); ridisegna(); } catch (e) { avvisa(testoErrore(e)); }
}

/** Provider e modelli della macchina: dal campo `provider[]`, altrimenti `GET /modelli`. */
function providerDellaMacchina(m) {
  const box = el('div.provider-macchina');
  const elencoP = (Array.isArray(m.provider) ? m.provider.map((p) => (typeof p === 'string' ? { id: p, nome: p } : p)) : []).filter((p) => providerAcceso(p.id)); // 2.L: Codex spento non compare
  if (!elencoP.length) { box.append(el('small', {}, delServer(m) ? 'Provider del server: vedi «Modelli e quote».' : 'Nessun client trovato su questa macchina.')); return box; }
  for (const p of elencoP) {
    const modelli = el('small', {}, '…');
    box.append(el('div.riga-elenco', {}, icona('anello', 14), el('span.riga-testi', {},
      el('span', {}, el('strong', {}, p.nome || p.id), p.versione ? el('small', {}, ` ${p.versione}`) : null), modelli)));
    // solo il conteggio («263 modelli»): l'elenco intero si apre nel foglio di scelta
    const scrivi = (ms) => {
      const n = ms.filter((x) => x && etichettaModello(x, p)).length;
      modelli.replaceChildren(n ? `${n} modell${n === 1 ? 'o' : 'i'} ` : 'nessun modello', n ? el('button.tasto-link', {
        type: 'button', 'aria-label': `Mostra tutti i modelli di ${p.nome || p.id} su ${m.nome}`,
        onclick: () => { leggiMacchine().then(() => apriSceltaModello({ tutte: true, soloVedi: true, titolo: `${m.nome} · ${p.nome || p.id}`, limita: (v) => v.macchina.id === m.id && v.provider?.id === p.id })); },
      }, 'mostra tutti') : null);
    };
    if (Array.isArray(p.modelli)) scrivi(p.modelli);
    else modelliDi(p.id).then(scrivi).catch(() => scrivi([]));
  }
  return box;
}

export async function paginaComputer({ ridisegna }) {
  const macchine = elenco(await api.macchine(), 'macchine').filter((m) => m.stato !== 'rimossa');
  leggiMacchine();
  const pcw = await infoPcWindows();
  const righe = macchine.map((m) => {
    const radici = Array.isArray(m.radici) ? m.radici : [];
    const capacita = Array.isArray(m.capacita) ? m.capacita : [];
    const posti = m.posti_liberi !== undefined && m.posti_liberi !== null ? `${m.posti_liberi}/${m.posti ?? 1} posti liberi` : `${m.posti ?? 1} posti`;
    const dettagli = [m.genere, m.os && m.arch ? `${m.os} ${m.arch}` : m.os, m.versione_lavoratore ? `lavoratore ${m.versione_lavoratore}` : null].filter(Boolean).join(' · ');
    return el('div.macchina', {},
      el('div.provider-riga', {},
        el('span.icona-tonda', {}, icona(ICONE[m.genere] || 'computer', 18)),
        el('span.testi', {}, el('strong', {}, m.nome, delServer(m) ? el('small.etichetta-server', {}, ' server') : null),
          el('small', {}, [STATI[m.stato] || m.stato, m.ultimo_battito_ms ? `battito ${quandoBreve(m.ultimo_battito_ms)}` : 'mai visto', posti].join(' · ')),
          dettagli ? el('small', { style: 'display:block' }, dettagli) : null),
        el(`span.punto-${inLinea(m) ? 'verde' : 'grigio'}`, { 'aria-label': STATI[m.stato] || m.stato, role: 'img' })),
      capacita.length ? el('div.capacita', { 'aria-label': 'Capacità' }, ...capacita.map((c) => el('span.pastiglia', {}, CAPACITA[c] || c))) : null,
      providerDellaMacchina(m),
      el('div.coppia', {}, el('span', {}, 'Esecuzione su questa macchina'),
        tendina(`Permesso di esecuzione su ${m.nome}`, PERMESSI, m.permesso_esecuzione || 'ask', async (v) => {
          try { await api.permessoMacchina(m.id, v); avvisa(`${m.nome}: ${PERMESSI.find((p) => p[0] === v)[1].toLowerCase()}`); } catch (e) { avvisa(testoErrore(e)); ridisegna(); }
        })),
      radici.length ? el('div.radici', {}, el('small', {}, 'Cartelle concesse'),
        ...radici.map((r) => el('div.riga-elenco', {}, icona('file', 14), el('span.riga-testi', {}, el('code', {}, r.percorso || r), el('small', {}, r.permessi || ''))))) : null,
      delServer(m) ? null : el('div.riga-tasti', {}, el('span.spazio'),
        el('button.tasto.piccolo.pericolo', { type: 'button', onclick: () => rimuovi(m, ridisegna) }, icona('cestino', 14), 'Rimuovi')));
  });
  return [
    el('div.riquadro-titolo', {}, 'Dispositivi'),
    el('div.riquadro', {}, righe.length ? righe : vuoto('Nessuna macchina registrata: il server si registra da solo all\'avvio.')),
    pcw ? el('div.riquadro-titolo', {}, 'Terminali') : null,
    pcw ? el('div.riquadro', {}, el('div.macchina', {}, el('div.provider-riga', {},
      el('span.icona-tonda', {}, icona('computer', 18)),
      el('span.testi', {}, el('strong', {}, pcw.nome || 'Computer Windows'), el('small', {}, 'PC dell\'amministrazione · terminale e schermo, a pieno schermo')),
      el('button.tasto.piccolo', { type: 'button', onclick: () => apriPcWindows(pcw, 'terminale') }, icona('tastiera', 14), 'Terminale'),
      el('button.tasto.piccolo', { type: 'button', onclick: () => apriPcWindows(pcw, 'schermo') }, icona('computer', 14), 'Schermo')))) : null,
    el('button.tasto', { type: 'button', onclick: () => collega(ridisegna) }, icona('piu', 14), 'Collega un computer'),
    nota('La VPS resta il centro: telefono e Mac parlano con lei. Il Mac collegato esegue i turni di Cursor e Grok, e di Claude Code quando serve Chrome o i file del Mac. Se il Mac è spento, «Auto» ripiega sulla VPS con Claude Code.'),
    nota('«Consenti sempre» esegue senza chiedere; «Chiedi ogni volta» manda una richiesta in chat; «Mai» blocca i comandi su quella macchina.'),
  ];
}
