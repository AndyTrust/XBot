// Le rotte della Fase 2.I (CONTRATTO-2I.md, sezioni 2 e 3), aggiunte al client `api`:
// OAuth dei collegamenti, entra/esci e interruttore dei provider, eliminazione dei canali revocati.

import { api, chiama, enc, q } from './api.js';

Object.assign(api, {
  // 2.1 OAuth dei collegamenti
  avviaOauth: (id) => chiama('POST', `/collegamenti/${enc(id)}/oauth/avvia`, {}),
  scollegaOauth: (id) => chiama('DELETE', `/collegamenti/${enc(id)}/oauth`),
  // server senza registrazione dinamica (Slack): client_id e segreto dell'app dell'utente
  clientOauth: (id, dati) => chiama('PUT', `/collegamenti/${enc(id)}/oauth/client`, dati),
  collegamento: (id) => chiama('GET', `/collegamenti/${enc(id)}`),

  // 3.2 Account dei provider
  accediProvider: (id, macchina_id) => chiama('POST', `/provider/${enc(id)}/accedi`, { macchina_id }),
  statoAccesso: (id, macchina_id) => chiama('GET', `/provider/${enc(id)}/accesso${q({ macchina_id })}`),
  // Claude Code: il codice che la pagina di accesso mostra va incollato qui
  codiceAccesso: (id, macchina_id, codice) => chiama('POST', `/provider/${enc(id)}/accesso`, { macchina_id, codice }),
  esciProvider: (id, macchina_id) => chiama('POST', `/provider/${enc(id)}/esci`, { macchina_id }),
  modificaProvider: (id, dati) => chiama('PATCH', `/provider/${enc(id)}`, dati),

  // 4.3 Canali: DELETE su un canale già revocato lo elimina (S, crates/db/src/canali.rs)
  eliminaCanale: (id) => chiama('DELETE', `/canali/${enc(id)}`),
});
