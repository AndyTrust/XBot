// Modelli e quote (CONTRATTO-2I sez. 4.2): per ogni provider il pallino (attivo e almeno una macchina
// in linea), l'interruttore «attivo», una riga per macchina con versione e account, «Entra» (link o
// comando, stato ogni 3 s) ed «Esci» (con conferma), l'anello della quota o i token stimati.

import { api, testoErrore } from '../api.js';
import { anello, icona } from '../icone.js';
import { barreQuota, fraseScaduta, quotaScaduta } from './quota-barre.js';
import { etichettaMacchina, inLinea } from '../macchine.js';
import { chiediConferma } from '../menu.js';
import { caricaProvider, caricaQuote, modelliDi, nomeProvider, providerAuto, quotaDi } from '../stato.js';
import { coppia, daCopiare, foglio, interruttore } from '../ui.js';
import { avvisa, compatto, el, fraQuanto, numero } from '../util.js';

/** L'anello della quota; per `quota_fonte = 'stimata'` i token dei 7 giorni, mai una percentuale inventata. */
export function anelloGrande(providerId, fonte) {
  const q = quotaDi(providerId);
  if (fonte === 'stimata') {
    const n = Number(q?.token_ingresso_7g || 0) + Number(q?.token_uscita_7g || 0);
    return el('span.anello-grande.stimata', {}, anello(null, 22, 3), `quota non dichiarata dal client · ${compatto(n)} token in 7 giorni`);
  }
  const u = q?.percentuale_usata;
  if (u === null || u === undefined) return el('span.anello-grande', {}, anello(null, 22, 3), 'quota non letta');
  // 2.L difetto 6: finestra già azzerata (`scaduta` dal server o `azzera_ms` passato): niente percentuale vecchia
  if (quotaScaduta(q)) return el('span.anello-grande.scaduta', {}, anello(null, 22, 3), fraseScaduta(q));
  const bassa = 100 - u < 10;
  return el(`span.anello-grande${bassa ? '.bassa' : ''}`, {}, anello(u, 22, 3),
    `${numero(u)} % usata${q.azzera_ms ? ` · si azzera ${fraQuanto(q.azzera_ms)}` : ''}`);
}

/** «nome@esempio.it (Max)» da `account` = {nome, dettaglio} (o un testo semplice). */
function testoAccount(a) {
  if (!a) return null;
  if (typeof a === 'string') return a;
  const d = a.dettaglio ? ` (${a.dettaglio.charAt(0).toUpperCase()}${a.dettaglio.slice(1)})` : '';
  return a.nome ? `${a.nome}${d}` : null;
}

const ESITI = { in_attesa: 'aspetto che tu finisca l\'accesso…', riuscito: 'accesso riuscito', fallito: 'l\'accesso non è riuscito', scaduto: 'il tempo è scaduto: premi di nuovo «Entra»', uscito: 'nessun accesso in corso: premi di nuovo «Entra»', terminale: 'il client vuole il Terminale: chiudi e premi di nuovo «Entra»' };

/** «Entra»: il link da aprire con lo stato che si rilegge ogni 3 s, o il comando da incollare sul Mac. */
async function entra(p, m, ridisegna) {
  // `codex login` toglie subito l'accesso che c'era (S §7): «Entra» solo senza account, e qui si ricontrolla.
  if (m.account) { avvisa(`${p.nome} è già collegato su ${etichettaMacchina(m)}: prima «Esci».`); return; }
  let r;
  try { r = await api.accediProvider(p.id, m.id); } catch (e) { avvisa(`${p.nome}: l'accesso non parte. ${testoErrore(e)}`, 5000); return; }
  if (r?.modo === 'terminale') {
    foglio({ titolo: `Entra in ${p.nome} su ${etichettaMacchina(m)}`, unaColonna: true, corpo: [
      el('p.largo', {}, 'Apri il Terminale sul Mac e incollalo:'), el('div.largo', {}, daCopiare(r.comando || '')),
      el('p.nota.largo', {}, 'Finito l\'accesso, la riga della macchina mostra l\'account entro mezzo minuto.'),
    ] });
    setTimeout(ridisegna, 30000);
    return;
  }
  if (!r?.url) { avvisa(`${p.nome}: il client non ha dato il link di accesso.`, 5000); return; }
  const riga = el('p.stato-accesso.largo', { role: 'status' }, ESITI.in_attesa);
  // Claude Code: la pagina dà un codice da incollare qui (`chiede_codice`), mandato con POST …/accesso.
  const campoCodice = el('input', { type: 'text', autocomplete: 'off', spellcheck: 'false', 'aria-label': 'Codice della pagina di accesso', placeholder: 'incolla il codice' });
  const mandaCodice = el('button.tasto.piccolo', { type: 'button', onclick: async () => {
    const codice = campoCodice.value.trim();
    if (!codice) { campoCodice.focus(); return; }
    mandaCodice.disabled = true;
    try { await api.codiceAccesso(p.id, m.id, codice); riga.textContent = 'codice mandato: controllo…'; } catch (e) { riga.textContent = testoErrore(e); }
    mandaCodice.disabled = false;
  } }, 'Manda il codice');
  const f = foglio({ titolo: `Entra in ${p.nome} su ${etichettaMacchina(m)}`, unaColonna: true, corpo: [
    el('p.largo', {}, 'Apri la pagina, entra con il tuo account e torna qui.'),
    el('a.tasto.primario.largo', { href: r.url, target: '_blank', rel: 'noopener' }, icona('destra', 14), 'Apri la pagina di accesso'),
    el('div.largo', {}, el('small', {}, 'Il link, se lo apri da un altro dispositivo'), daCopiare(r.url)),
    r.codice_utente ? el('div.largo', {}, el('small', {}, 'Codice da scrivere nella pagina'), daCopiare(r.codice_utente)) : null,
    r.chiede_codice ? el('div.largo.codice-accesso', {}, el('small', {}, 'Il codice che la pagina ti mostra alla fine'), el('div.da-copiare', {}, campoCodice, mandaCodice)) : null,
    r.scade_s ? el('p.nota.largo', {}, `Il link vale ${Math.round(r.scade_s / 60) || 1} minuti.`) : null,
    riga,
  ] });
  const giro = async () => {
    if (!f.nodo.isConnected) return;
    let s;
    try { s = await api.statoAccesso(p.id, m.id); } catch (e) { riga.textContent = testoErrore(e); setTimeout(giro, 3000); return; }
    riga.textContent = ESITI[s?.stato] || s?.stato || ESITI.in_attesa;
    riga.dataset.stato = s?.stato || '';
    if (s?.stato === 'riuscito') {
      const a = testoAccount(s.account);
      riga.textContent = a ? `Accesso riuscito: ${a}` : ESITI.riuscito;
      avvisa(`${p.nome}: ${riga.textContent}`);
      ridisegna();
      return;
    }
    if (s?.stato === 'in_attesa' || s?.stato === null || s?.stato === undefined) setTimeout(giro, 3000);
  };
  setTimeout(giro, 3000);
}

async function esci(p, m, ridisegna) {
  const mac = m.genere === 'mac';
  if (!await chiediConferma({ titolo: `Uscire da ${p.nome} su ${etichettaMacchina(m)}?`,
    testo: mac ? 'Dovrai rientrare dal Mac.' : 'Dovrai rientrare con «Entra».', conferma: 'Esci', pericolo: true })) return;
  try { await api.esciProvider(p.id, m.id); avvisa(`${p.nome}: uscito da ${etichettaMacchina(m)}`); ridisegna(); } catch (e) { avvisa(testoErrore(e)); }
}

function rigaMacchina(p, m, ridisegna) {
  const vivo = inLinea(m);
  const a = testoAccount(m.account);
  const pezzi = [etichettaMacchina(m), vivo ? 'connesso' : 'spento', m.versione, a || 'non collegato'].filter(Boolean);
  return el('div.coppia.macchina-provider', { 'data-macchina': m.id },
    el('span', {}, el('span', { class: `pallino ${vivo ? 'verde' : 'grigio'}`, 'aria-hidden': 'true' }), pezzi.join(' · ')),
    a ? el('button.tasto.piccolo', { type: 'button', onclick: () => esci(p, m, ridisegna) }, 'Esci')
      : el('button.tasto.piccolo.primario', { type: 'button', disabled: !vivo, title: vivo ? '' : 'La macchina è spenta', onclick: () => entra(p, m, ridisegna) }, 'Entra'));
}

/** L'interruttore «attivo» di un provider (anche nel riquadro «Spenti» per riaccenderlo). */
const interruttoreProvider = (p, ridisegna) => interruttore(`${p.nome} attivo`, !!p.attivo, async (si, n) => {
  try { await api.modificaProvider(p.id, { attivo: si }); await caricaProvider(); avvisa(si ? `${p.nome} attivo` : `${p.nome} spento: non compare più`); ridisegna(); } catch (e) { n.checked = !si; avvisa(testoErrore(e)); }
});

export async function paginaModelli({ ridisegna }) {
  await caricaQuote();
  const tutti = await api.provider();
  // 2.L difetto 9: i provider spenti (Codex) escono dall'elenco e restano solo nel riquadro «Spenti»
  const acceso = (p) => p.attivo !== 0 && p.attivo !== false;
  const elenco = tutti.filter(acceso);
  const spenti = tutti.filter((p) => !acceso(p));
  const righe = await Promise.all(elenco.map(async (p) => {
    let m = [];
    try { m = await modelliDi(p.id); } catch { m = []; }
    const macchine = p.macchine || [];
    const vive = macchine.filter(inLinea);
    const verde = !!p.attivo && vive.length > 0;
    const frase = !p.attivo ? 'spento' : vive.length ? `in linea su ${vive.map(etichettaMacchina).join(', ')}` : 'macchina spenta';
    return el('div.provider-blocco', { 'data-provider': p.id },
      el('div.provider-riga', {},
        el('span', { class: `pallino-grande ${verde ? 'verde' : 'grigio'}`, 'aria-hidden': 'true' }, verde ? '●' : '○'),
        el('span.testi', {}, el('strong', {}, p.nome),
          el('small.stato-provider', { style: 'display:block' }, frase),
          el('small', {}, m.length ? `modelli: ${m.slice(0, 6).map((x) => x.etichetta).join(' · ')}${m.length > 6 ? ` e altri ${m.length - 6}` : ''}` : 'nessun modello')),
        interruttoreProvider(p, ridisegna)),
      el('div.provider-quota', {}, anelloGrande(p.id, p.quota_fonte), barreQuota(p.id, p.quota_fonte)),
      macchine.length ? el('div.provider-macchine', {}, macchine.map((x) => rigaMacchina(p, x, ridisegna)))
        : el('p.nota', {}, 'Nessuna macchina ha questo client.'));
  }));
  const auto = providerAuto();
  return [
    el('div.riquadro-titolo', {}, 'Provider'),
    el('div.riquadro', {}, righe.length ? righe : el('p.nota', {}, 'Nessun provider.')),
    spenti.length ? el('div.riquadro-titolo', {}, 'Spenti') : null,
    spenti.length ? el('div.riquadro.provider-spenti', {}, spenti.map((p) => el('div.coppia', { 'data-provider': p.id },
      el('span', {}, el('strong', {}, p.nome), el('small', {}, 'spento: non compare nella scelta, in Utilizzo, in Computer e in Squadra')), interruttoreProvider(p, ridisegna)))) : null,
    el('div.riquadro-titolo', {}, 'Scelta automatica'),
    el('div.riquadro', {}, coppia('«Auto» sceglie', el('strong', {}, auto ? nomeProvider(auto) : 'nessun provider attivo'),
      'Claude Code se è attivo, altrimenti il primo provider attivo')),
    el('p.nota', {}, 'I provider nascono dai client trovati sulle macchine; ognuno usa il tuo abbonamento con il suo client ufficiale. Regola fissa: se il modello scelto non c\'è, errore. Mai un altro modello in silenzio.'),
  ];
}
