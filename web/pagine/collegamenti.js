// Collegamenti (Layout sez. 3, CONTRATTO-2I sez. 4.1): server MCP per gli assistenti. Catalogo con un
// solo tasto «Collega» (OAuth nella stessa scheda, foglio «Token» o prova subito), i miei con la riga di
// stato in italiano, acceso/spento, Prova, Modifica, Account (Ricollega, Scollega), strumenti e permessi,
// assistenti esclusi, Rimuovi. Si apre anche dal tasto della barra.
// 2.K: client OAuth prima del consenso, «Dove gira» (tendina Macchina, «gira su Mac», «Mac spento»),
// prova che scopre l'OAuth (401) → «account da collegare» con «Collega account».
// 2.L: «Spento per questi assistenti» (esclusi), menu Account secondo lo stato, «Istruzioni» per Google,
// «aggiunto» per id della voce, voci `tecnico` senza comando.

import { api, elenco, testoErrore } from '../api.js';
import { icona } from '../icone.js';
import { leggiMacchine } from '../macchine.js';
import { apriMenu, chiediConferma } from '../menu.js';
import { stato } from '../stato.js';
import { interruttore, nota, tastino, tendina, vuoto } from '../ui.js';
import { avvisa, el, quandoBreve } from '../util.js';
import {
  avviaOauth, datiDaVoce, doveGira, foglioCampi, foglioClient, foglioIstruzioni, foglioModifica, foglioPersonalizzato, foglioToken, fraseMotivo, fraseNonRiceve, logoVoce, providerDiAssistente,
  scelteMacchina, serveClient, sottotitolo,
} from './collegamenti-fogli.js';

const PERMESSI = [['sempre', 'sempre'], ['chiedi', 'chiedi'], ['bloccato', 'bloccato']];
const CATEGORIE = [['', 'Tutti'], ['nostri', 'I nostri'], ['terzi', 'Di terzi'], ['personalizzati', 'Personalizzati']];
let categoria = '';
let cerca = '';

/** La voce del catalogo di un mio collegamento: `voce_id`/`catalogo_id` se il server li dà, altrimenti
 *  lo stesso url (http) o lo stesso comando con gli stessi argomenti (stdio). Mai per nome. */
function voceDi(c, cat) {
  const id = c.voce_id || c.catalogo_id;
  if (id) return cat.find((x) => x.id === id) || null;
  if (c.trasporto === 'http') return cat.find((x) => x.url && x.url === c.url) || null;
  const comando = [c.comando, ...(c.argomenti || [])].join(' ');
  return cat.find((x) => x.trasporto !== 'http' && x.comando && [x.comando, ...(x.argomenti || [])].join(' ') === comando) || null;
}

/** 2.L: una voce `tecnico` (Posta IMAP) non mostra il comando ma «sul Mac · le caselle dell'utente». */
function rigaTecnica(c) {
  if (!c.voce?.tecnico) return null;
  const dove = doveGira(c);
  const cosa = String(c.voce.descrizione || '').split(':')[0].trim();
  return [`sul ${dove ? dove.nome : 'server'}`, cosa].filter(Boolean).join(' · ');
}

/** OAuth: 'ok' | 'scaduto' | 'da_collegare' (richiede l'account e non c'è) | null (non serve). */
function statoAccount(c) {
  if (c.oauth?.stato === 'ok') return 'ok';
  if (c.oauth?.stato === 'scaduto') return 'scaduto';
  return c.richiede_oauth ? 'da_collegare' : null;
}

function dettagli(c) {
  const strumenti = c.strumenti || [];
  const permessi = {};
  const salvaPermessi = async (k, v) => {
    permessi[k] = v;
    try { await api.permessiCollegamento(c.id, { ...permessi }); avvisa('Permesso salvato'); } catch (e) { avvisa(testoErrore(e)); }
  };
  // 2.L: un collegamento acceso vale per tutti i bot; le caselle dicono per chi è spento (`esclusi`).
  const esclusi = new Set(c.esclusi || []);
  const salvaEsclusi = async () => {
    try { await api.esclusiCollegamento(c.id, [...esclusi]); c.esclusi = [...esclusi]; avvisa(esclusi.size ? `Spento per ${esclusi.size} ${esclusi.size === 1 ? 'assistente' : 'assistenti'}` : 'Acceso per tutti gli assistenti'); } catch (e) { avvisa(testoErrore(e)); }
  };
  const casella = (a) => {
    const p = providerDiAssistente(a);
    return el(`label.casella${p.riceve ? '' : '.disattiva'}`, { title: p.riceve ? '' : fraseNonRiceve(p) },
      el('input', { type: 'checkbox', checked: esclusi.has(a.id), onchange: (ev) => { if (ev.target.checked) esclusi.add(a.id); else esclusi.delete(a.id); salvaEsclusi(); } }),
      el('span', {}, a.nome, p.riceve ? null : el('small.non-riceve', {}, fraseNonRiceve(p))));
  };
  return el('div.voce-extra', {},
    el('small.sotto-titolo', {}, 'Strumenti e permessi'),
    el('div.coppia', {}, el('span', {}, 'Predefinito per le letture'), tendina('Permesso predefinito per le letture', PERMESSI, 'sempre', (v) => salvaPermessi('*lettura', v))),
    el('div.coppia', {}, el('span', {}, 'Predefinito per le scritture'), tendina('Permesso predefinito per le scritture', PERMESSI, 'chiedi', (v) => salvaPermessi('*scrittura', v))),
    ...(strumenti.length ? strumenti.map((s) => el('div.coppia', {}, el('span', {}, el('code', {}, s.nome), s.descrizione ? el('small', {}, s.descrizione) : null),
      tendina(`Permesso di ${s.nome}`, PERMESSI, s.permesso || 'chiedi', (v) => salvaPermessi(s.nome, v)))) : [vuoto('Nessuno strumento letto: premi «Prova».')]),
    el('small.sotto-titolo', {}, 'Spento per questi assistenti'),
    el('p.nota', {}, 'Nessuna casella: vale per tutti. Segna chi non lo deve usare.'),
    el('div.caselle', {}, [...stato.assistenti.values()].filter((a) => !a.archiviato_ms).map(casella)));
}

/** L'errore di una prova: una frase nostra e il testo tecnico dietro «Mostra dettagli». */
function erroreProva(c, tecnico) {
  const account = /collegamento dell'account|non_autenticato|401/i.test(tecnico || '');
  const spento = /non è collegato/i.test(tecnico || ''); // S 2.K: 200 ok:false «il computer «mac» non è collegato: accendilo e riprova»
  const frase = account ? 'Serve il collegamento dell\'account: premi «Collega account».'
    : spento ? `${doveGira(c)?.nome || 'Il computer'} non è collegato: accendilo e riprova.`
    : c.trasporto === 'http' ? 'Il server non risponde all\'indirizzo dato.' : 'Il comando non risponde come server MCP.';
  return el('span.errore-prova', {}, el('span', {}, frase),
    tecnico ? el('details', {}, el('summary', {}, 'Mostra dettagli'), el('code', {}, tecnico)) : null);
}

/** La riga di stato sotto il nome: «gira su Mac · collegato come … · 12 strumenti · provato 3 min fa». */
function rigaStato(c) {
  const dove = doveGira(c);
  if (dove && !dove.acceso) return el('span.stato-collegamento.rosso', {}, `gira su ${dove.nome} · ${dove.nome} spento: la prova e i turni aspettano che torni in linea`);
  const testo = rigaStatoAccount(c);
  return dove ? el('span.con-dove', {}, el('span.stato-collegamento.dove', {}, `gira su ${dove.nome} ·`), testo) : testo;
}

function rigaStatoAccount(c) {
  const acc = statoAccount(c);
  if (acc === 'scaduto') return el('span.stato-collegamento.rosso', {}, 'accesso scaduto: ricollega');
  if (acc === 'da_collegare') return el('span.stato-collegamento.attesa', {}, 'account da collegare: premi «Collega account»');
  if (c.ultimo_errore) return erroreProva(c, c.ultimo_errore);
  const pezzi = [];
  if (acc === 'ok') pezzi.push(c.oauth.account ? `collegato come ${c.oauth.account}` : 'account collegato');
  if (c.ultimo_ok_ms) pezzi.push(`${(c.strumenti || []).length} strumenti`, `provato ${quandoBreve(c.ultimo_ok_ms)}`);
  else pezzi.push('mai provato');
  return el('span.stato-collegamento', {}, pezzi.join(' · '));
}

async function prova(c, esito) {
  esito.replaceChildren('provo…');
  try {
    const r = await api.provaCollegamento(c.id);
    if (r?.ok) {
      c.strumenti = r.strumenti || []; c.ultimo_errore = null; c.ultimo_ok_ms = Date.now();
      esito.replaceChildren(rigaStato(c));
      avvisa(`«${c.nome}» risponde: ${c.strumenti.length} strumenti`);
    } else {
      c.ultimo_errore = r?.errore || 'nessuna risposta';
      // MCP generico: un 401 fa segnare al server `richiede_oauth`; rileggo la riga per saperlo.
      if (c.trasporto === 'http' && !c.richiede_oauth && /collegamento dell'account|401/i.test(c.ultimo_errore)) {
        try { Object.assign(c, await api.collegamento(c.id)); } catch { c.richiede_oauth = true; }
      }
      esito.replaceChildren(erroreProva(c, c.ultimo_errore));
    }
    return r;
  } catch (e) { esito.replaceChildren(testoErrore(e)); return null; }
}

/** Il menu «Account» secondo lo stato (2.L difetto 3): senza account «Collega account», «Client OAuth…» (solo
 *  se `richiede_client`), «Istruzioni» (client nel server, Google); con account «Ricollega» e «Scollega». */
export function vociAccount(c, ridisegna) {
  const collegato = !!c.oauth && c.oauth.stato !== 'revocato';
  const istruzioni = c.client_nel_server ? { testo: 'Istruzioni', icona: 'info', nota: 'il client sta nel suo server', azione: () => foglioIstruzioni(c) } : null;
  if (!collegato) {
    return [
      { titolo: 'Account non collegato' },
      { testo: 'Collega account', icona: 'presa', azione: () => avviaOauth(c, { chiedi: false }) },
      c.richiede_client ? { testo: 'Client OAuth…', icona: 'chiave', nota: c.client_registrato ? 'registrato' : 'da inserire', azione: () => foglioClient(c, ridisegna) } : null,
      istruzioni,
    ].filter(Boolean);
  }
  return [
    { titolo: c.oauth.account || 'Account collegato' },
    { testo: 'Ricollega', icona: 'aggiorna', azione: () => avviaOauth(c) },
    { testo: 'Scollega', icona: 'chiudi', azione: async () => {
      if (!await chiediConferma({ titolo: `Scollegare l'account di «${c.nome}»?`, testo: 'Il token si cancella e il collegamento si spegne. Per usarlo di nuovo serve un nuovo consenso.', conferma: 'Scollega', pericolo: true })) return;
      try { await api.scollegaOauth(c.id); avvisa('Account scollegato'); ridisegna(); } catch (e) { avvisa(testoErrore(e)); }
    } },
    istruzioni,
  ].filter(Boolean);
}

const menuAccount = (c, ridisegna) => (ev) => apriMenu(ev.currentTarget, vociAccount(c, ridisegna));

function riga(c, ridisegna, provaSubito) {
  const box = el('div.voce-cassaforte.collegamento', { 'data-id': c.id });
  let aperto = null;
  const esito = el('small.esito-prova', {}, rigaStato(c));
  const dove = doveGira(c);
  const spento = dove && !dove.acceso;
  const acc = statoAccount(c);
  const tProva = tastino(spento ? `${dove.nome} spento` : 'Prova', 'play', async (ev) => {
    const t = ev.currentTarget;
    t.disabled = true;
    await prova(c, esito);
    if (statoAccount(c) !== acc) { box.replaceWith(riga(c, ridisegna)); return; } // 401 → «Collega account»
    if (aperto) { const n = dettagli(c); aperto.replaceWith(n); aperto = n; }
    t.disabled = false;
  }, spento ? { disabled: true, title: `${dove.nome} spento: la prova gira lì` } : {});
  box.append(
    el('div.riga-elenco', {},
      logoVoce(c.voce, el('span.icona-tonda', {}, icona(acc === 'ok' ? 'lucchetto' : 'presa', 16))),
      el('span.riga-testi.con-esito', {}, el('strong', {}, c.nome),
        el('small', {}, sottotitolo(c) || rigaTecnica(c) || (c.trasporto === 'http' ? (c.url || 'http') : [c.comando, ...(c.argomenti || [])].join(' '))), esito),
      interruttore(`«${c.nome}» acceso`, c.abilitato, async (si, n) => {
        try { await api.modificaCollegamento(c.id, { abilitato: si }); avvisa(si ? 'Acceso' : 'Spento'); } catch (e) { n.checked = !si; avvisa(testoErrore(e)); }
      })),
    el('div.riga-tasti', {},
      acc === 'da_collegare' || acc === 'scaduto' ? el('button.tasto.piccolo.primario', { type: 'button', onclick: () => avviaOauth(c, { chiedi: false }) }, icona('presa', 14), acc === 'scaduto' ? 'Ricollega' : 'Collega account') : null,
      tProva,
      tastino('Modifica', 'matita', () => foglioModifica(c, ridisegna)),
      tastino('Strumenti e assistenti', 'impostazioni', () => {
        if (aperto) { aperto.remove(); aperto = null; return; }
        aperto = dettagli(c);
        box.append(aperto);
      }),
      c.richiede_oauth || c.oauth ? tastino('Account', 'persona', menuAccount(c, ridisegna), { 'aria-haspopup': 'menu' }) : null,
      el('span.spazio'),
      tastino('Rimuovi', 'cestino', async () => {
        if (!await chiediConferma({ titolo: `Rimuovere «${c.nome}»?`, testo: 'Gli assistenti non potranno più usarlo.', conferma: 'Rimuovi', pericolo: true })) return;
        try { await api.togliCollegamento(c.id); avvisa('Rimosso'); ridisegna(); } catch (e) { avvisa(testoErrore(e)); }
      }, { class: 'tasto piccolo pericolo' })));
  if (provaSubito) setTimeout(() => tProva.click(), 0);
  return box;
}

/** «Collega» dal catalogo: crea, poi foglio «Client OAuth…» (Slack, Google), consenso OAuth, foglio «Token» o prova subito. */
async function collega(x, ridisegna, tasto, macchinaId) {
  if (x.richiede_token) { foglioToken(x, (dopo) => ridisegna({ prova: dopo?.id })); return; }
  if (x.campi?.length) { foglioCampi(x, (dopo) => ridisegna({ prova: dopo?.id }), macchinaId); return; }
  tasto.disabled = true;
  let r;
  const extra = x.trasporto !== 'http' && macchinaId ? { macchina_id: macchinaId } : {};
  try { r = await api.creaCollegamento(datiDaVoce(x, extra)); } catch (e) { tasto.disabled = false; avvisa(`«${x.nome}» non si crea: ${testoErrore(e)}`, 5000); return; }
  if (x.richiede_oauth && r?.id) {
    const nuovo = { id: r.id, nome: x.nome, url: x.url, richiede_client: !!x.richiede_client, client_istruzioni: x.client_istruzioni, client_registrato: false };
    if (serveClient(nuovo)) ridisegna(); // la riga c'è anche se il foglio si chiude senza salvare
    await avviaOauth(nuovo, { chiedi: false });
    tasto.disabled = false;
    return;
  }
  avvisa(`«${x.nome}» aggiunto: lo provo`);
  ridisegna({ prova: r?.id });
}

/** L'avviso del ritorno dal consenso (`?oauth=ok|errore`). */
function avvisoRitorno(o, miei) {
  if (!o?.oauth) return null;
  const c = miei.find((x) => x.id === o.collegamento);
  const nome = c ? `«${c.nome}»` : 'Il collegamento';
  if (o.oauth === 'ok') {
    const testo = `${nome}: account collegato${c?.oauth?.account ? ` come ${c.oauth.account}` : ''}. Lo provo adesso.`;
    avvisa(testo, 4000);
    return el('p.avviso-forte.ok.ritorno-oauth', { role: 'status' }, icona('spunta', 16), testo);
  }
  const testo = `${nome}: ${fraseMotivo(o.motivo)}`;
  avvisa(testo, 5000);
  return el('div.avviso-forte.errore.ritorno-oauth', { role: 'alert' }, icona('avviso', 16), el('span', {}, testo,
    o.motivo ? el('details', {}, el('summary', {}, 'Mostra dettagli'), el('code', {}, o.motivo)) : null));
}

export async function paginaCollegamenti({ ridisegna, opzioni = {} }) {
  const [miei, cat] = await Promise.all([api.collegamenti().then((r) => elenco(r, 'collegamenti')), api.catalogo().then((r) => elenco(r, 'catalogo')).catch(() => []), leggiMacchine()]);
  for (const c of miei) { // dalla voce del catalogo quello che la riga non porta (client, istruzioni, `tecnico`)
    const v = voceDi(c, cat);
    c.voce = v || null;
    if (v && c.trasporto === 'http' && c.richiede_client === undefined) c.richiede_client = !!v.richiede_client;
    if (v && c.client_nel_server === undefined) c.client_nel_server = !!v.client_nel_server;
    if (v && !c.client_istruzioni) c.client_istruzioni = v.client_istruzioni;
  }
  const daProvare = opzioni.prova || (opzioni.oauth === 'ok' ? opzioni.collegamento : null);
  const ridisegnaCon = (o) => (o?.prova ? ridisegna({ prova: o.prova }) : ridisegna());
  const passa = (x) => (!categoria || (x.categoria || 'personalizzati') === categoria) && (!cerca || `${x.nome} ${x.descrizione || ''}`.toLowerCase().includes(cerca));
  const vociMie = new Set(miei.map((c) => c.voce?.id).filter(Boolean)); // «aggiunto» per id della voce, non per nome
  const campoCerca = el('input', { type: 'search', placeholder: 'Cerca tra i collegamenti', value: cerca, 'aria-label': 'Cerca tra i collegamenti' });
  let timer;
  campoCerca.addEventListener('input', () => { clearTimeout(timer); timer = setTimeout(() => { cerca = campoCerca.value.trim().toLowerCase(); ridisegna(); }, 300); });
  if (cerca) setTimeout(() => { const c = document.querySelector('.cerca-collegamenti input'); if (c) { c.focus(); c.setSelectionRange(c.value.length, c.value.length); } }, 0);
  const accesi = miei.filter((c) => c.abilitato).length;
  const voceCatalogo = (x) => {
    const stdio = x.trasporto !== 'http';
    const scelta = stdio ? scelteMacchina(x.macchina) : null;
    let macchinaId = scelta?.valore || '';
    const tMacchina = stdio ? tendina(`Macchina di «${x.nome}»`, scelta.opzioni, macchinaId, (v) => { macchinaId = v; }) : null;
    const tasto = el('button.tasto.piccolo.primario', { type: 'button', onclick: (ev) => collega(x, ridisegnaCon, ev.currentTarget, macchinaId) }, 'Collega');
    const come = x.richiede_client ? 'serve il client OAuth di un\'app tua' : x.richiede_oauth ? 'accesso con il tuo account' : x.richiede_token ? 'serve un token' : x.campi?.length ? `chiede ${x.campi.map((c) => c.etichetta.charAt(0).toLowerCase() + c.etichetta.slice(1)).join(', ')}` : null;
    const aggiunto = vociMie.has(x.id);
    return el('div.riga-elenco.voce-catalogo', { 'data-id': x.id },
      logoVoce(x, el('span.icona-tonda', {}, icona(x.richiede_oauth ? 'lucchetto' : 'presa', 16))),
      el('span.riga-testi', {}, el('strong', {}, x.nome), sottotitolo(x) ? el('small.sottotitolo', {}, sottotitolo(x)) : null,
        el('small', {}, [x.descrizione, come].filter(Boolean).join(' · ')), x.nota ? el('small', {}, x.nota) : null),
      aggiunto ? el('span.etichetta', {}, 'aggiunto') : el('span.riga-tasti.collega-voce', {}, tMacchina, tasto));
  };
  return [
    avvisoRitorno(opzioni, miei),
    el('div.riga-tasti.testa-pagina', {}, el('label.cerca-cassaforte.cerca-collegamenti', {}, icona('cerca', 16), campoCerca), el('span.spazio'),
      el('button.tasto', { type: 'button', onclick: () => foglioPersonalizzato(ridisegna) }, icona('piu', 14), 'Personalizzato')),
    el('div.schede-pillola', { role: 'tablist' }, CATEGORIE.map(([k, t]) => el('button', { type: 'button', role: 'tab', 'aria-selected': categoria === k ? 'true' : 'false', onclick: () => { categoria = k; ridisegna(); } }, t))),
    el('div.riquadro-titolo', {}, `I miei · ${accesi} accesi`),
    el('div.riquadro.elenco', {}, miei.filter(passa).length ? miei.filter(passa).map((c) => riga(c, ridisegna, c.id === daProvare)) : vuoto('Nessun collegamento: premi «Collega» su una voce del catalogo.')),
    el('div.riquadro-titolo', {}, 'Catalogo'),
    el('div.riquadro.elenco', {}, cat.filter(passa).length ? cat.filter(passa).map(voceCatalogo) : vuoto('Nessuna voce del catalogo.')),
    nota('Un collegamento acceso entra nei turni di tutti gli assistenti, su Claude Code, Grok e Cursor, salvo quelli segnati in «Spento per questi assistenti». «Bloccato» nega lo strumento, «chiedi» manda una richiesta in chat.'),
  ];
}
