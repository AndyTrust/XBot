// Le rotte della Fase 2.D (CONTRATTO-2D.md, sezioni 1–8), aggiunte al client `api`.

import { api, chiama, chiamaBinario, enc, q } from './api.js';

const file = (campo, f, extra = {}) => {
  const m = new FormData();
  m.append(campo, f, f.name || campo);
  for (const [k, v] of Object.entries(extra)) if (v !== undefined && v !== null) m.append(k, v);
  return m;
};

Object.assign(api, {
  // 1. Cassaforte
  // il testo cercato va nel corpo (POST), mai nell'indirizzo: non finisce nei log
  cassaforte: (genere, cerca) => (cerca ? chiama('POST', '/cassaforte/cerca', { testo: cerca, genere: genere || undefined }) : chiama('GET', `/cassaforte${q({ genere: genere || undefined })}`)),
  creaVoce: (dati) => chiama('POST', '/cassaforte', dati),
  modificaVoce: (id, dati) => chiama('PATCH', `/cassaforte/${enc(id)}`, dati),
  togliVoce: (id) => chiama('DELETE', `/cassaforte/${enc(id)}`),
  rivela: (id) => chiama('POST', `/cassaforte/${enc(id)}/rivela`, {}),
  importaCsv: (f) => chiama('POST', '/cassaforte/importa', file('file', f)),
  usi: (id) => chiama('GET', `/cassaforte/${enc(id)}/usi`),

  // 2. Assistenti
  caricaAvatar: (id, f) => chiama('POST', `/assistenti/${enc(id)}/avatar`, file('immagine', f)),
  togliAvatar: (id) => chiama('DELETE', `/assistenti/${enc(id)}/avatar`),
  profilo: (id) => chiama('GET', `/assistenti/${enc(id)}/profilo`),
  scriviProfilo: (id, testo) => chiama('PUT', `/assistenti/${enc(id)}/profilo`, { testo }),
  abilita: (id) => chiama('GET', `/assistenti/${enc(id)}/abilita`),
  unaAbilita: (id, nome) => chiama('GET', `/assistenti/${enc(id)}/abilita/${enc(nome)}`),
  creaAbilita: (id, dati) => chiama('POST', `/assistenti/${enc(id)}/abilita`, dati),
  modificaAbilita: (id, nome, dati) => chiama('PATCH', `/assistenti/${enc(id)}/abilita/${enc(nome)}`, dati),
  togliAbilita: (id, nome) => chiama('DELETE', `/assistenti/${enc(id)}/abilita/${enc(nome)}`),
  routine: (id) => chiama('GET', `/assistenti/${enc(id)}/routine`),
  creaRoutine: (id, dati) => chiama('POST', `/assistenti/${enc(id)}/routine`, dati),
  modificaRoutine: (rid, dati) => chiama('PATCH', `/routine/${enc(rid)}`, dati),
  togliRoutine: (rid) => chiama('DELETE', `/routine/${enc(rid)}`),
  eseguiRoutine: (rid) => chiama('POST', `/routine/${enc(rid)}/esegui`, {}),
  memoria: (id) => chiama('GET', `/assistenti/${enc(id)}/memoria`),
  libreria: (id) => chiama('GET', `/assistenti/${enc(id)}/file`),
  urlBlob: (hash) => `/api/v1/blob/${enc(hash)}`,

  // allegati e spostamento della sessione
  allega: (sessione, f) => chiama('POST', `/sessioni/${enc(sessione)}/allegati`, file('file', f)),
  sposta: (sessione, macchina_id) => chiama('POST', `/sessioni/${enc(sessione)}/sposta`, { macchina_id }),

  // 4. Richieste, permessi, regole
  richieste: (stato = 'aperta') => chiama('GET', `/richieste${q({ stato })}`),
  risolvi: (id, corpo) => chiama('POST', `/richieste/${enc(id)}/risolvi`, corpo),
  permessi: () => chiama('GET', '/permessi'),
  revocaPermesso: (id) => chiama('DELETE', `/permessi/${enc(id)}`),
  regole: () => chiama('GET', '/regole-revisione'),
  scriviRegole: (regole) => chiama('PUT', '/regole-revisione', { regole }),

  // 8. Macchine
  macchine: () => chiama('GET', '/macchine'),
  pcWindows: () => chiama('GET', '/pc-windows'),
  permessoMacchina: (id, permesso_esecuzione) => chiama('PUT', `/macchine/${enc(id)}/permesso`, { permesso_esecuzione }),
  biglietto: () => chiama('POST', '/macchine/biglietto', {}),
  lavori: (id, limite = 10) => chiama('GET', `/macchine/${enc(id)}/lavori${q({ limite })}`),
  // 2.E: macchine collegate, squadra, turno in attesa del Mac
  togliMacchina: (id) => chiama('DELETE', `/macchine/${enc(id)}`),
  squadra: () => chiama('GET', '/squadra'),
  eseguiSullaVps: (turnoId) => chiama('POST', `/turni/${enc(turnoId)}/sulla-vps`, {}),
  // 2.E-bis: la chiamata vocale finita e il voto stanno sul server (righe della sessione)
  salvaChiamata: (sid, dati) => chiama('POST', `/sessioni/${enc(sid)}/chiamate`, dati),
  vota: (rigaId, voto) => chiama('POST', `/righe/${enc(rigaId)}/voto`, { voto }),
  // sessioni annidate e lavagna comune
  figlie: (sid) => chiama('GET', `/sessioni/${enc(sid)}/figlie`),
  apriFiglia: (sid, dati) => chiama('POST', `/sessioni/${enc(sid)}/figlie`, dati),
  lavagna: () => chiama('GET', '/lavagna'),
  chiudiVoce: (id) => chiama('POST', `/lavagna/${enc(id)}/chiudi`, {}),
  // 2.H: il computer dell'assistente sulla VPS (Xvfb + noVNC + ttyd)
  computer: (aid) => chiama('GET', `/assistenti/${enc(aid)}/computer`),
  azioneComputer: (aid, azione) => chiama('POST', `/assistenti/${enc(aid)}/computer/${azione}`, {}),

  // 5. Collegamenti
  collegamenti: () => chiama('GET', '/collegamenti'),
  catalogo: () => chiama('GET', '/collegamenti/catalogo'),
  creaCollegamento: (dati) => chiama('POST', '/collegamenti', dati),
  modificaCollegamento: (id, dati) => chiama('PATCH', `/collegamenti/${enc(id)}`, dati),
  togliCollegamento: (id) => chiama('DELETE', `/collegamenti/${enc(id)}`),
  provaCollegamento: (id) => chiama('POST', `/collegamenti/${enc(id)}/prova`, {}),
  esclusiCollegamento: (id, esclusi) => chiama('PUT', `/collegamenti/${enc(id)}/esclusi`, { esclusi }), // 2.L: i bot per cui è spento
  permessiCollegamento: (id, permessi) => chiama('PUT', `/collegamenti/${enc(id)}/permessi`, { permessi }),

  // 6. Canali
  canali: () => chiama('GET', '/canali'),
  creaCanale: (dati) => chiama('POST', '/canali', dati),
  revocaCanale: (id) => chiama('DELETE', `/canali/${enc(id)}`),
  rigeneraCanale: (id) => chiama('POST', `/canali/${enc(id)}/rigenera`, {}),
  chiamate: (id, limite = 20) => chiama('GET', `/canali/${enc(id)}/chiamate${q({ limite })}`),

  // 7. Voce
  trascrivi: (audio, lingua) => chiama('POST', '/trascrivi', file('audio', audio, { lingua })),
  sintesi: (testo, voce_id, velocita) => chiamaBinario('POST', '/sintesi', { testo, voce_id: voce_id || undefined, velocita: velocita || undefined }),
  voci: () => chiama('GET', '/voci'),

  // 8. Utilizzo, versione, eventi, push
  utilizzo: (filtri = {}) => chiama('GET', `/utilizzo${q(filtri)}`),
  versione: () => chiama('GET', '/versione'),
  eventi: (limite = 50) => chiama('GET', `/eventi${q({ limite })}`),
  chiavePush: () => chiama('GET', '/push/chiave'),
  abbonaPush: (dati) => chiama('POST', '/push/abbonamento', dati),
});
