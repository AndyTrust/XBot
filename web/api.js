// Il client dell'API v1: JSON, errori { errore: { codice, testo } }, cookie di sessione.

export const BASE = '/api/v1';

/** Errore del server con il codice stabile (accesso_negato, troppi_tentativi, ...). */
export class ErroreApi extends Error {
  constructor(codice, testo, stato) {
    super(testo || codice);
    this.codice = codice;
    this.stato = stato;
  }
}

/** Errore di rete: il server non ha risposto (si riprova). */
export class ErroreRete extends Error {
  constructor(causa) {
    super('il server non risponde');
    this.codice = 'rete';
    this.causa = causa;
  }
}

let quandoFuori = () => {};
/** Chi chiamare quando l'accesso scade (401 non_autorizzato). */
export function aScadenzaAccesso(f) { quandoFuori = f; }

export async function chiama(metodo, rotta, corpo) {
  const modulo = corpo instanceof FormData;
  const r = await prendi(metodo, rotta, {
    headers: corpo === undefined || modulo ? { accept: 'application/json' } : { 'content-type': 'application/json', accept: 'application/json' },
    body: corpo === undefined ? undefined : (modulo ? corpo : JSON.stringify(corpo)),
  });
  let dati = null;
  const testo = await r.text();
  if (testo) {
    try { dati = JSON.parse(testo); } catch { dati = null; }
  }
  if (!r.ok) {
    const e = dati?.errore;
    const err = new ErroreApi(e?.codice || `http_${r.status}`, e?.testo || r.statusText, r.status);
    if (err.codice === 'non_autorizzato' && rotta !== '/io') quandoFuori(err);
    throw err;
  }
  return dati;
}

async function prendi(metodo, rotta, opzioni) {
  try {
    return await fetch(BASE + rotta, { method: metodo, credentials: 'same-origin', ...opzioni });
  } catch (e) {
    throw new ErroreRete(e);
  }
}

/** Una risposta binaria (audio della sintesi): restituisce il Blob. */
export async function chiamaBinario(metodo, rotta, corpo) {
  const r = await prendi(metodo, rotta, { headers: { 'content-type': 'application/json' }, body: JSON.stringify(corpo) });
  if (!r.ok) {
    let e = null;
    try { e = (await r.json())?.errore; } catch { /* corpo non JSON */ }
    throw new ErroreApi(e?.codice || `http_${r.status}`, e?.testo || r.statusText, r.status);
  }
  return r.blob();
}

/** Un elenco da una risposta che può essere {chiave: [...]} oppure già un array. */
export function elenco(r, chiave) {
  if (Array.isArray(r)) return r;
  if (r && Array.isArray(r[chiave])) return r[chiave];
  return [];
}

export const q = (o) => {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(o)) if (v !== undefined && v !== null) p.set(k, v);
  const s = p.toString();
  return s ? `?${s}` : '';
};
export const enc = encodeURIComponent;

export const api = {
  salute: () => chiama('GET', '/salute'),
  entra: (password, codice_totp, dispositivo, fidato = false) => chiama('POST', '/accesso', { password, codice_totp, dispositivo, fidato }),
  dispositiviFidati: () => chiama('GET', '/dispositivi-fidati'),
  revocaFidato: (id) => chiama('DELETE', `/dispositivi-fidati/${enc(id)}`),
  revocaFidatiTutti: () => chiama('DELETE', '/dispositivi-fidati'),
  esci: () => chiama('DELETE', '/accesso'),
  io: () => chiama('GET', '/io'),
  barra: () => chiama('GET', '/barra'),
  assistenti: () => chiama('GET', '/assistenti'),
  assistente: (id) => chiama('GET', `/assistenti/${enc(id)}`),
  creaAssistente: (dati) => chiama('POST', '/assistenti', dati),
  modificaAssistente: (id, dati) => chiama('PATCH', `/assistenti/${enc(id)}`, dati),
  archiviaAssistente: (id) => chiama('DELETE', `/assistenti/${enc(id)}`),
  sezioni: () => chiama('GET', '/sezioni'),
  creaSezione: (nome, ordine = 0) => chiama('POST', '/sezioni', { nome, ordine }),
  modificaSezioni: (elenco) => chiama('PATCH', '/sezioni', elenco),
  sessioni: (assistente, tutte = false) => chiama('GET', `/assistenti/${enc(assistente)}/sessioni${q({ tutte: tutte || undefined })}`),
  creaSessione: (assistente, dati = {}) => chiama('POST', `/assistenti/${enc(assistente)}/sessioni`, dati),
  sessione: (id) => chiama('GET', `/sessioni/${enc(id)}`),
  modificaSessione: (id, dati) => chiama('PATCH', `/sessioni/${enc(id)}`, dati),
  azzera: (id) => chiama('POST', `/sessioni/${enc(id)}/azzera`),
  righe: (id, dopo, generazione, limite) => chiama('GET', `/sessioni/${enc(id)}/righe${q({ dopo_aggiornata_seq: dopo, generazione, limite })}`),
  urlFlusso: (id, dopo, generazione) => `${BASE}/sessioni/${enc(id)}/flusso${q({ dopo_aggiornata_seq: dopo, generazione })}`,
  invia: (sessione, invio) => chiama('POST', `/sessioni/${enc(sessione)}/invii`, invio),
  interrompi: (turno) => chiama('POST', `/turni/${enc(turno)}/interrompi`),
  quote: () => chiama('GET', '/quote'),
  provider: () => chiama('GET', '/provider'),
  modelli: (provider) => chiama('GET', `/modelli${q({ provider })}`),
  // Contratto: {"valori": {chiave: valore}} in lettura e in scrittura.
  impostazioni: async () => (await chiama('GET', '/impostazioni'))?.valori || {},
  scriviImpostazioni: async (valori) => (await chiama('PUT', '/impostazioni', { valori }))?.valori || null,
  letto: (assistente, fino_seq = 0) => chiama('POST', `/assistenti/${enc(assistente)}/letto`, { fino_seq }),
};

/** Testo per l'utente da un errore qualunque. */
export function testoErrore(e) {
  const noti = {
    accesso_negato: 'Password o codice sbagliati.',
    totp_richiesto: 'Questo dispositivo non è fidato: serve il codice.',
    troppi_tentativi: 'Troppi tentativi falliti: riprova fra qualche minuto.',
    non_autorizzato: 'Serve di nuovo l\'accesso.',
    rete: 'Il server non risponde. Riprovo da solo.',
    modello_non_disponibile: 'Il modello scelto non è disponibile.',
    provider_fermo: 'Il provider è fermo.',
    turno_in_corso: 'C\'è un turno in corso.',
    sessione_archiviata: 'La sessione è archiviata.',
    rotta_non_trovata: 'Il server non ha ancora questa funzione.',
    http_404: 'Il server non ha ancora questa funzione.',
    carta_non_valida: 'Il numero della carta non è valido.',
    macchina_non_connessa: 'La macchina non è connessa.',
    richiesta_chiusa: 'La richiesta è già chiusa.',
    richiesta_gia_risolta: 'La richiesta è già stata risolta.',
    tipo_non_accettato: 'Formato non accettato dal server: usa png, jpg o webp.',
    cassaforte_chiusa: 'La cassaforte è chiusa sul server.',
    voce_non_trovata: 'La voce non c\'è più.',
    push_non_configurato: 'Le notifiche non sono configurate sul server (chiavi VAPID).',
    macchina_non_trovata: 'La macchina non esiste.',
    macchina_del_server: 'La macchina del server non si può rimuovere.',
    turno_non_in_attesa: 'Il turno non è più in attesa del Mac.',
    biglietto_scaduto: 'Il biglietto è scaduto: chiedine uno nuovo.',
    voce_non_configurata: 'La voce non è configurata sul server (chiave fish.audio).',
    variabile_non_ammessa: 'Quel nome di variabile è riservato al sistema.',
    testo_non_valido: 'Il testo contiene caratteri non ammessi.',
    credito_voce_finito: 'Il credito di fish.audio è finito: la voce non parla.',
    voce_fallita: 'Il motore della voce sul server non ha risposto.',
    corpo_troppo_grande: 'Il file è troppo grande.',
    // 2.I
    oauth_stato_non_valido: 'Il tentativo di collegamento è scaduto: premi di nuovo «Collega».',
    oauth_non_richiesto: 'Questo collegamento non usa l\'accesso con l\'account.',
    canale_attivo: 'Prima revoca il canale, poi lo elimini.',
    provider_non_trovato: 'Il provider non esiste.',
  };
  return noti[e?.codice] || e?.message || 'Errore sconosciuto.';
}
