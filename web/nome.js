// Il nome visibile della app: una sola costante. Per cambiarlo: python3 web/prove/rinomina.py <nome>
// Se il server lo dice (`nome_app` in GET /salute, il nome del bot scelto all'installazione), vince quello.
export let NOME_APP = "XBot";

/** Mette il nome detto dal server nella costante, nel titolo e nella pagina di accesso. */
export function usaNomeApp(nome) {
  const n = String(nome || '').trim().slice(0, 60);
  if (!n || n === NOME_APP) return;
  const prima = NOME_APP;
  NOME_APP = n;
  document.title = document.title === prima ? n : document.title.replace(` · ${prima}`, ` · ${n}`);
  const h = document.getElementById('nome-app');
  if (h) h.textContent = n;
}
