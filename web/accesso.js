// La pagina di accesso (Layout sez. 8): password, nome del dispositivo, casella del dispositivo fidato;
// il codice TOTP compare solo se il server lo chiede (totp_richiesto) o se l'utente tocca «Ho un codice».
// Il cookie fidato è HttpOnly: il client prova sempre prima senza codice.

import { api, testoErrore } from './api.js';
import { NOME_APP } from './nome.js';
import { $, locale } from './util.js';

/** Mostra l'accesso; `dopo` parte quando l'accesso riesce. */
export function mostraAccesso(dopo, motivo) {
  $('#finestra').hidden = true;
  const pagina = $('#accesso');
  pagina.hidden = false;
  const modulo = $('#modulo-accesso');
  const errore = $('#accesso-errore');
  const disp = modulo.elements.dispositivo;
  const fidato = modulo.elements.fidato;
  const rigaTotp = $('#riga-totp');
  const hoCodice = $('#tasto-ho-codice');
  const totp = modulo.elements.codice_totp;
  const mostraTotp = (si) => { rigaTotp.hidden = !si; hoCodice.hidden = si; if (!si) totp.value = ''; };
  modulo.elements.password.value = '';
  mostraTotp(false);
  if (!disp.value) disp.value = locale.leggi('dispositivo', '') || indovinaDispositivo();
  fidato.checked = locale.leggi('fidato', true) !== false; // 2026-10-25: spuntata di base, il codice una volta sola
  if (motivo) { errore.textContent = motivo; errore.hidden = false; } else errore.hidden = true;
  modulo.elements.password.focus();

  hoCodice.onclick = () => { mostraTotp(true); totp.focus(); };
  fidato.onchange = () => locale.scrivi('fidato', fidato.checked);

  modulo.onsubmit = async (ev) => {
    ev.preventDefault();
    const tasto = modulo.querySelector('button[type=submit]');
    const password = modulo.elements.password.value;
    const codice = rigaTotp.hidden ? '' : totp.value.replace(/\s/g, '');
    const dispositivo = disp.value.trim();
    errore.hidden = true;
    tasto.disabled = true;
    tasto.textContent = 'Entro…';
    try {
      await api.entra(password, codice || null, dispositivo || null, fidato.checked);
      locale.scrivi('dispositivo', dispositivo);
      await salvaPassword(modulo);
      // niente reset: il gestore delle password del browser legge il modulo quando la pagina sparisce
      pagina.hidden = true;
      mostraTotp(false);
      dopo();
    } catch (e) {
      if (e.codice === 'totp_richiesto') {
        const giaChiesto = !rigaTotp.hidden && codice;
        mostraTotp(true);
        errore.textContent = giaChiesto ? 'Serve il codice di 6 cifre.' : 'Questo dispositivo non è fidato: serve il codice.';
        errore.dataset.codice = e.codice;
        errore.hidden = false;
        totp.focus();
        return;
      }
      // accesso_negato e troppi_tentativi in chiaro, come li dà il server
      errore.textContent = testoErrore(e);
      errore.dataset.codice = e.codice || '';
      errore.hidden = false;
      totp.value = '';
      (e.codice === 'accesso_negato' ? modulo.elements.password : tasto).focus();
    } finally {
      tasto.disabled = false;
      tasto.textContent = 'Entra';
    }
  };
}

/** Chrome e simili: chiede di salvare la password. Safari e Firefox non hanno PasswordCredential: niente. */
async function salvaPassword(modulo) {
  try {
    if (!('PasswordCredential' in window) || !navigator.credentials?.store) return;
    // il campo utente è nascosto e vuoto: il gestore delle password vuole un nome, si prende quello del server
    const utente = modulo.elements.utente;
    if (!utente.value) utente.value = (await api.io().catch(() => null))?.nome || NOME_APP;
    // eslint-disable-next-line no-undef
    await navigator.credentials.store(new PasswordCredential(modulo));
  } catch { /* ripiego silenzioso */ }
}

function indovinaDispositivo() {
  const ua = navigator.userAgent;
  if (/Android/i.test(ua)) return 'Telefono Android';
  if (/iPhone|iPad/i.test(ua)) return 'iPhone';
  if (/Macintosh/i.test(ua)) return 'Mac';
  return 'Browser';
}
