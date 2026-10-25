// Fogli dei collegamenti (CONTRATTO-2I sez. 4.1): personalizzato, «Modifica» con i nomi delle
// intestazioni e delle variabili (mai i valori) e «sostituisci», «Token» per le voci che lo chiedono,
// avvio dell'OAuth nella stessa scheda, frasi del ritorno dal consenso, chi riceve i collegamenti.
// 2.K: foglio «Client OAuth…» prima del consenso (Slack) e tendina «Macchina» per i comandi stdio.
// 2.L: «Istruzioni» per Google (`client_nel_server`), esclusioni al posto delle abilitazioni.

import { api, testoErrore } from '../api.js';
import { icona } from '../icone.js';
import { chiediConferma } from '../menu.js';
import { etichettaMacchina, inLinea, macchinaServer, macchine } from '../macchine.js';
import { nomeProvider, providerAuto, stato } from '../stato.js';
import { campo, daCopiare, foglio } from '../ui.js';
import { avvisa, el } from '../util.js';

export const coppieDa = (t, sep) => Object.fromEntries((t || '').split('\n').map((r) => r.trim()).filter(Boolean).map((r) => {
  const i = r.indexOf(sep);
  return i < 0 ? [r, ''] : [r.slice(0, i).trim(), r.slice(i + 1).trim()];
}));

/** Dove si prende il token di una voce del catalogo che lo chiede. */
const DOVE_TOKEN = { github: 'https://github.com/settings/personal-access-tokens' };

/** Le voci che vogliono un client OAuth registrato a mano (Slack, Google) e non l'hanno ancora. */
export const serveClient = (c) => !!c?.richiede_client && !c.client_registrato;

/** La voce Google del catalogo e i suoi collegamenti: sotto il nome «Gmail · Drive · Calendar». */
export const sottotitolo = (x) => (x?.id === 'google' || /\/google-mcp\//.test(x?.url || '') ? 'Gmail · Drive · Calendar' : null);

/** «Dove gira» un collegamento stdio: `macchina_id` diverso dal server (null = sul server). */
export function doveGira(c) {
  if (!c?.macchina_id) return null;
  const m = macchine.find((x) => x.id === c.macchina_id);
  if (m && m === macchinaServer()) return null;
  return { id: c.macchina_id, nome: m ? etichettaMacchina(m) : c.macchina_id, acceso: inLinea(m) };
}

/** La macchina suggerita dal catalogo (`macchina`: id o genere, es. `mac`). */
const macchinaSuggerita = (s) => (s ? macchine.find((m) => m.id === s) || macchine.find((m) => m.genere === s) || null : null);

/** Le scelte della tendina «Macchina»: il server (valore vuoto) e le macchine in linea; la suggerita anche se spenta. */
export function scelteMacchina(suggerita) {
  const server = macchinaServer();
  const sug = macchinaSuggerita(suggerita);
  const altre = macchine.filter((m) => m !== server && m.stato !== 'rimossa' && (inLinea(m) || m === sug));
  const opzioni = [['', `${etichettaMacchina(server)} (server)`], ...altre.map((m) => [m.id, inLinea(m) ? etichettaMacchina(m) : `${etichettaMacchina(m)} (spento)`])];
  return { opzioni, valore: sug && sug !== server ? sug.id : '' };
}

/** Il corpo di `POST /collegamenti` da una voce del catalogo: solo i campi che il server accetta. */
export function datiDaVoce(x, extra = {}) {
  const http = x.trasporto === 'http';
  return {
    nome: x.nome, trasporto: x.trasporto || 'stdio', categoria: x.categoria || 'personalizzati',
    ...(http ? { url: x.url, argomenti: [] } : { comando: x.comando, argomenti: x.argomenti || [] }),
    ...extra,
  };
}

/** Il provider con cui lavora un assistente (Auto = quello che sceglie il server) e se riceve i collegamenti. */
export function providerDiAssistente(a) {
  const id = a?.provider_predefinito_id || providerAuto();
  const p = stato.provider.find((x) => x.id === id);
  const riceve = p && p.riceve_collegamenti !== undefined && p.riceve_collegamenti !== null ? !!p.riceve_collegamenti : id === 'claude_code';
  return { id, nome: nomeProvider(id), riceve };
}

/** La frase accanto alla casella disattivata. */
export const fraseNonRiceve = (p) => `usa ${p.nome}: questo provider non riceve i collegamenti`;

/** Chiede l'URL del consenso e ci va nella stessa scheda: al ritorno l'app riapre Collegamenti. */
export async function avviaOauth(c, { chiedi = true } = {}) {
  // 2.K: Slack e Google vogliono il client dell'utente prima del primo tentativo; dopo «Salva» parte il consenso.
  if (serveClient(c)) { foglioClient(c, () => avviaOauth({ ...c, client_registrato: true }, { chiedi: false })); return; }
  if (chiedi && c.oauth?.stato === 'ok' && !await chiediConferma({
    titolo: `Ricollegare «${c.nome}»?`, testo: `L'account ${c.oauth.account || 'collegato'} viene sostituito da quello con cui entri adesso.`, conferma: 'Ricollega',
  })) return;
  let r;
  try { r = await api.avviaOauth(c.id); } catch (e) {
    // Ripiego: niente registrazione automatica e `richiede_client` assente (S: 502 oauth_fallito «registrazione…»).
    // 2.L: Google ha il client nel suo server (`client_nel_server`): un 502 vuol dire server Google spento o da configurare.
    if (c.client_nel_server && (e?.stato === 502 || e?.codice === 'oauth_fallito')) { foglioIstruzioni(c, true); return; }
    if (e?.codice === 'oauth_fallito' && /registrazione/i.test(e.message || '')) {
      avvisa(`«${c.nome}» non accetta la registrazione automatica: inserisci il client OAuth.`, 5000);
      foglioClient(c, () => avviaOauth(c, { chiedi: false }));
      return;
    }
    avvisa(`«${c.nome}»: il consenso non parte. ${testoErrore(e)}`, 5000);
    return;
  }
  if (!r?.url) { avvisa(`«${c.nome}»: il server non ha dato l'indirizzo del consenso.`, 5000); return; }
  avvisa('Ti porto alla pagina del consenso…');
  location.assign(r.url);
}

/** Gli indirizzi di ritorno da dare all'app del client: il nostro e, per Google, quello del suo server MCP. */
function indirizziRitorno(c) {
  const lista = [['Indirizzo di ritorno di XBot', `${location.origin}/api/v1/oauth/ritorno`]];
  const g = /^(https?:\/\/[^/]+)(?:\/[^]*)?\/google-mcp\//.exec(c.url || '');
  if (g) lista.push(['Indirizzo di ritorno del server Google', `${g[1]}/google-mcp/oauth2callback`]);
  return lista;
}

/** 2.L: «Istruzioni» per le voci con il client nel loro server (Google): il testo del server e l'indirizzo
 *  di ritorno da mettere nella console di Google. Nessun campo: il client non passa da XBot. */
export function foglioIstruzioni(c, dopoErrore = false) {
  const righe = (c.client_istruzioni || 'Il client OAuth di questo collegamento si configura nel suo server, non in XBot.')
    .split(/\n+/).map((r) => r.trim()).filter(Boolean);
  const g = /^(https?:\/\/[^/]+)(?:\/[^]*)?\/google-mcp\//.exec(c.url || '');
  foglio({
    titolo: `Come si configura «${c.nome}»`, unaColonna: true,
    corpo: [
      dopoErrore ? el('p.avviso-forte.errore.largo', { role: 'alert' }, icona('avviso', 16), 'Il consenso non parte: il server di questo collegamento è spento o non ha ancora il client. Segui questi passi.') : null,
      el('div.largo.istruzioni-client', {}, righe.map((r) => el('p.nota', {}, r))),
      g ? el('div.largo.ritorno-client', {}, el('small.sotto-titolo', {}, 'Indirizzo di ritorno da mettere nella console di Google'), daCopiare(`${g[1]}/google-mcp/oauth2callback`)) : null,
    ],
  });
}

/** «Client OAuth…»: client_id e segreto di un'app registrata a mano (`PUT …/oauth/client`). Il segreto non si rilegge mai. */
export function foglioClient(c, dopo) {
  const salvato = c.client_registrato ?? !!c.oauth;
  const istruzioni = (c.client_istruzioni || `Crea un'app OAuth per «${c.nome}» e metti fra gli indirizzi di ritorno autorizzati quello qui sotto.`)
    .split(/\n+/).map((r) => r.trim()).filter(Boolean);
  const segreto = el('input', { name: 'client_secret', type: 'password', style: 'flex:1;min-width:0', autocomplete: 'off', spellcheck: 'false', hidden: salvato, placeholder: 'il segreto dell\'app' });
  const pallini = el('span.valore-nascosto', { hidden: !salvato }, '••••');
  const sostituisci = el('button.tasto.piccolo', { type: 'button', hidden: !salvato, onclick: () => { segreto.hidden = false; pallini.hidden = true; sostituisci.hidden = true; segreto.focus(); } }, 'sostituisci');
  foglio({
    titolo: `Client OAuth di «${c.nome}»`, conferma: 'Salva', unaColonna: true,
    corpo: [
      el('div.largo.istruzioni-client', {}, istruzioni.map((r) => el('p.nota', {}, r))),
      ...indirizziRitorno(c).map(([t, u]) => el('div.largo.ritorno-client', {}, el('small.sotto-titolo', {}, t), daCopiare(u))),
      campo('Client ID', { name: 'client_id', required: true, autocomplete: 'off', spellcheck: 'false', maxlength: 500 }, true),
      el('label.campo-modulo.largo', {}, 'Segreto del client', el('span.riga-tasti.segreto-client', {}, pallini, sostituisci, segreto)),
      el('p.nota.largo', {}, 'Il segreto resta cifrato sul server e non si rilegge più. Dopo «Salva» si apre la pagina del consenso.'),
    ],
    invia: async (m) => {
      const id = m.querySelector('[name=client_id]').value.trim();
      if (!id) throw new Error('Serve il Client ID.');
      const dati = { client_id: id };
      if (!segreto.hidden && segreto.value.trim()) dati.client_secret = segreto.value.trim();
      try { await api.clientOauth(c.id, dati); } catch (e) { throw new Error(testoErrore(e)); }
      avvisa(`«${c.nome}»: client salvato`);
      dopo?.();
    },
  });
}

/** Frase per l'utente dal `motivo` del ritorno `?oauth=errore`. */
export function fraseMotivo(motivo) {
  const noti = {
    access_denied: 'hai negato il consenso.', accesso_negato: 'hai negato il consenso.',
    oauth_stato_non_valido: 'il tentativo è scaduto (vale 10 minuti): premi di nuovo «Collega».',
    scambio_fallito: 'il server dell\'account non ha dato il token.', token_non_ricevuto: 'il server dell\'account non ha dato il token.',
    registrazione_fallita: 'il server dell\'account non accetta la registrazione di XBot.',
    metadati_non_trovati: 'il server non dice dove si fa l\'accesso.',
    oauth_fallito: 'il server dell\'account ha rifiutato lo scambio.',
  };
  return noti[motivo] || 'il collegamento non è riuscito.';
}

/** Collegamento personalizzato (stdio o http), anche dalla voce del catalogo come base. */
export function foglioPersonalizzato(dopo, base = {}) {
  const trasporto = campo('Tipo', { name: 'trasporto', valore: base.trasporto || 'stdio', opzioni: [['stdio', 'Comando (stdio)'], ['http', 'HTTP']] });
  const perStdio = [
    campo('Comando', { name: 'comando', valore: base.comando || '', placeholder: 'es. npx', spellcheck: 'false' }),
    campo('Argomenti (uno per riga)', { name: 'argomenti', valore: (base.argomenti || []).join('\n'), rows: 3, spellcheck: 'false' }, true),
    campo('Variabili d\'ambiente (NOME=valore, una per riga)', { name: 'ambiente', rows: 2, spellcheck: 'false', placeholder: 'cifrate con la chiave della cassaforte' }, true),
    campo('Macchina (dove gira il comando)', { name: 'macchina_id', ...scelteMacchina(base.macchina) }),
  ];
  const perHttp = [
    campo('URL', { name: 'url', type: 'url', valore: base.url || '', placeholder: 'https://…' }, true),
    campo('Intestazioni (Nome: valore, una per riga)', { name: 'intestazioni', rows: 2, spellcheck: 'false', placeholder: 'Authorization: Bearer …' }, true),
  ];
  const mostra = () => {
    const http = trasporto.querySelector('select').value === 'http';
    perStdio.forEach((n) => { n.hidden = http; });
    perHttp.forEach((n) => { n.hidden = !http; });
  };
  trasporto.querySelector('select').addEventListener('change', mostra);
  foglio({
    titolo: 'Collegamento personalizzato', conferma: 'Aggiungi',
    corpo: [
      campo('Nome', { name: 'nome', valore: base.nome || '', required: true, maxlength: 80 }),
      trasporto, ...perStdio, ...perHttp,
      campo('Istruzioni per gli assistenti (facoltative)', { name: 'istruzioni', rows: 2, valore: base.istruzioni || '' }, true),
    ],
    invia: async (m) => {
      const v = Object.fromEntries(new FormData(m));
      const http = v.trasporto === 'http';
      if (http && !/^https?:\/\//.test(v.url || '')) throw new Error('Serve un URL http o https.');
      if (!http && !v.comando?.trim()) throw new Error('Serve il comando.');
      const dati = {
        nome: v.nome.trim(), trasporto: v.trasporto, categoria: 'personalizzati', istruzioni: v.istruzioni || undefined,
        ...(http ? { url: v.url.trim(), intestazioni: coppieDa(v.intestazioni, ':'), argomenti: [] }
          : { comando: v.comando.trim(), argomenti: (v.argomenti || '').split('\n').map((x) => x.trim()).filter(Boolean), ambiente: coppieDa(v.ambiente, '='),
            ...(v.macchina_id ? { macchina_id: v.macchina_id } : {}) }),
      };
      try { await api.creaCollegamento(dati); } catch (e) { throw new Error(testoErrore(e)); }
      avvisa(`«${dati.nome}» aggiunto: provalo con «Prova»`);
      dopo();
    },
  });
  mostra();
}

/** Voce del catalogo con `campi` (WordPress, Hostinger, …): token, password e indirizzi, cifrati sul server. */
export function foglioCampi(x, dopo, macchinaId) {
  foglio({
    titolo: `Collega «${x.nome}»`, conferma: 'Collega', unaColonna: true,
    corpo: [
      ...x.campi.map((c) => campo(c.etichetta, { name: c.nome, type: c.segreto ? 'password' : 'text', required: true, autocomplete: c.segreto ? 'new-password' : 'off', spellcheck: 'false' }, true)),
      ...x.campi.filter((c) => c.aiuto).map((c) => el('p.nota.largo', {}, `${c.etichetta}: `, /^https?:/.test(c.aiuto) ? el('a', { href: c.aiuto, target: '_blank', rel: 'noopener' }, c.aiuto) : c.aiuto)),
      x.nota ? el('p.nota.largo', {}, x.nota) : null,
      el('p.nota.largo', {}, 'Resta cifrato sul server: da qui in poi si vede solo il nome.'),
    ],
    invia: async (m) => {
      const d = new FormData(m);
      const ambiente = {}; const intestazioni = {};
      for (const c of x.campi) {
        const v = String(d.get(c.nome) || '').trim();
        if (!v) throw new Error(`Serve «${c.etichetta}».`);
        if (c.intestazione) intestazioni[c.nome] = `Bearer ${v}`; else ambiente[c.nome] = v;
      }
      const extra = { ...(Object.keys(ambiente).length ? { ambiente } : {}), ...(Object.keys(intestazioni).length ? { intestazioni } : {}), ...(x.trasporto !== 'http' && macchinaId ? { macchina_id: macchinaId } : {}) };
      let r;
      try { r = await api.creaCollegamento(datiDaVoce(x, extra)); } catch (e) { throw new Error(testoErrore(e)); }
      dopo({ id: r?.id, prova: true });
    },
  });
}

/** Il logo della voce (`web/loghi/<id>.png`); se il file non c'è (404) l'iniziale del servizio; senza logo l'icona data. */
export const logoVoce = (x, ic) => (x?.logo
  ? el('img.logo-collegamento', { src: `/app/${x.logo}`, alt: '', width: 32, height: 32, loading: 'lazy', onerror: (ev) => ev.currentTarget.replaceWith(logoLettera(x.nome || x.id)) })
  : ic);

/** L'iniziale del servizio in un cerchio, con un colore stabile ricavato dal nome. */
export function logoLettera(nome = '') {
  const n = String(nome).trim();
  let h = 0;
  for (const c of n.toLocaleLowerCase('it')) h = (h * 31 + c.codePointAt(0)) >>> 0;
  const lettera = (n.match(/[\p{L}\p{N}]/u)?.[0] || '?').toLocaleUpperCase('it');
  const s = el('span.logo-collegamento.logo-lettera', { 'aria-hidden': 'true' }, lettera);
  s.style.setProperty('--tinta', String(h % 360));
  return s;
}

/** Voce del catalogo con `richiede_token`: il solo campo «Token», che va in `Authorization: Bearer`. */
export function foglioToken(x, dopo) {
  const dove = x.token_url || DOVE_TOKEN[x.id];
  foglio({
    titolo: `Collega «${x.nome}»`, conferma: 'Collega', unaColonna: true,
    corpo: [
      campo('Token', { name: 'token', type: 'password', required: true, autocomplete: 'off', spellcheck: 'false' }, true),
      dove ? el('p.nota.largo', {}, 'Il token si crea qui: ', el('a', { href: dove, target: '_blank', rel: 'noopener' }, dove)) : null,
      el('p.nota.largo', {}, 'Resta cifrato sul server: da qui in poi si vede solo il nome dell\'intestazione.'),
    ],
    invia: async (m) => {
      const token = new FormData(m).get('token').trim();
      if (!token) throw new Error('Serve il token.');
      let r;
      try { r = await api.creaCollegamento(datiDaVoce(x, { intestazioni: { Authorization: `Bearer ${token}` } })); } catch (e) { throw new Error(testoErrore(e)); }
      dopo({ id: r?.id, prova: true });
    },
  });
}

/** Una riga «Nome ••••  [sostituisci] [togli]» per un'intestazione o una variabile già salvata. */
function rigaSegreta(nome, cambi, segnaposto) {
  const valore = el('input', { type: 'password', autocomplete: 'off', spellcheck: 'false', placeholder: segnaposto, hidden: true, 'aria-label': `Nuovo valore di ${nome}` });
  const testo = el('span.valore-nascosto', {}, '••••');
  const riga = el('div.coppia.segreta', {}, el('span', {}, el('code', {}, nome)), el('span.riga-tasti', {}, testo, valore,
    el('button.tasto.piccolo', { type: 'button', onclick: (ev) => {
      valore.hidden = false; testo.hidden = true; ev.currentTarget.hidden = true; valore.focus();
      valore.oninput = () => { cambi[nome] = valore.value; };
    } }, 'sostituisci'),
    el('button.tasto.piccolo.pericolo', { type: 'button', onclick: () => { cambi[nome] = null; riga.classList.add('tolta'); testo.textContent = 'si toglie al salvataggio'; } }, 'togli')));
  return riga;
}

/** «Modifica»: nome, istruzioni, indirizzo o comando, intestazioni e variabili per nome. */
export function foglioModifica(c, dopo) {
  const http = c.trasporto === 'http';
  const cambi = {};
  const nomi = (http ? c.intestazioni_nomi : c.ambiente_nomi) || [];
  const presenti = http ? c.intestazioni_presenti : c.ambiente_presente;
  const tipo = http ? 'Intestazioni' : 'Variabili d\'ambiente';
  foglio({
    titolo: `Modifica «${c.nome}»`, conferma: 'Salva',
    corpo: [
      campo('Nome', { name: 'nome', valore: c.nome, required: true, maxlength: 80 }, true),
      ...(http ? [campo('URL', { name: 'url', type: 'url', valore: c.url || '' }, true)]
        : [campo('Comando', { name: 'comando', valore: c.comando || '', spellcheck: 'false' }, true),
          campo('Argomenti (uno per riga)', { name: 'argomenti', valore: (c.argomenti || []).join('\n'), rows: 3, spellcheck: 'false' }, true)]),
      el('div.largo', {}, el('small.sotto-titolo', {}, tipo),
        nomi.length ? el('div', {}, nomi.map((n) => rigaSegreta(n, cambi, http ? 'nuovo valore' : 'nuovo valore')))
          : el('p.nota', {}, presenti ? 'Ce ne sono, ma il server non dice i nomi: aggiungine una con lo stesso nome per sostituirla.' : 'Nessuna.')),
      campo(http ? 'Aggiungi intestazioni (Nome: valore, una per riga)' : 'Aggiungi variabili (NOME=valore, una per riga)', { name: 'nuove', rows: 2, spellcheck: 'false' }, true),
      c.oauth?.stato === 'ok' ? el('p.nota.largo', {}, icona('lucchetto', 14), ' L\'accesso dell\'account lo aggiunge XBot a ogni turno: non serve scriverlo qui.') : null,
      campo('Istruzioni per gli assistenti', { name: 'istruzioni', rows: 2, valore: c.istruzioni || '' }, true),
    ],
    invia: async (m) => {
      const v = Object.fromEntries(new FormData(m));
      const dati = {};
      if (v.nome.trim() !== c.nome) dati.nome = v.nome.trim();
      if ((v.istruzioni || '') !== (c.istruzioni || '')) dati.istruzioni = v.istruzioni;
      if (http && v.url.trim() !== (c.url || '')) {
        if (!/^https?:\/\//.test(v.url.trim())) throw new Error('Serve un URL http o https.');
        dati.url = v.url.trim();
      }
      if (!http) {
        if (v.comando.trim() !== (c.comando || '')) dati.comando = v.comando.trim();
        const arg = v.argomenti.split('\n').map((x) => x.trim()).filter(Boolean);
        if (arg.join('\n') !== (c.argomenti || []).join('\n')) dati.argomenti = arg;
      }
      const mappa = { ...coppieDa(v.nuove, http ? ':' : '='), ...Object.fromEntries(Object.entries(cambi).filter(([, x]) => x === null || x !== '')) };
      if (Object.keys(mappa).length) dati[http ? 'intestazioni' : 'ambiente'] = mappa;
      if (!Object.keys(dati).length) { avvisa('Niente da salvare'); return; }
      try { await api.modificaCollegamento(c.id, dati); } catch (e) { throw new Error(testoErrore(e)); }
      avvisa(`«${dati.nome || c.nome}» salvato`);
      dopo();
    },
  });
}
