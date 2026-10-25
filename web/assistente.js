// Nuovo assistente e impostazioni dell'assistente (Layout sez. 5, CONTRATTO-2D sez. 2).
// Schede: Generale (avatar con forme, icone, personaggi, colori e foto; nome, titolo, descrizione,
// sezione, fissato, nascosto, colore in chat, memoria, archivia) · Modello e voce · Profilo ·
// Abilità · Collegamenti · Segreti · Routine.

import { api, testoErrore } from './api.js';
import { avatarSvg, caricaPersonaggi, COLORI, coloreDi, FORME, formaDi, fotoDi, ICONE, PERSONAGGI, personaggioPer } from './avatar.js';
import { apriSovrapposta, chiediConferma } from './menu.js';
import { sezioneAbilita } from './pagine/abilita.js';
import { schedaCollegamenti, schedaModelloVoce, schedaProfilo } from './pagine/assistente-schede.js';
import { sezioneRoutine } from './pagine/routine.js';
import { sezioneSegreti } from './pannello-schede.js';
import { schedaComputer } from './schermo.js';
import { assistenteDi, bus, caricaAssistenti, caricaBarra, stato } from './stato.js';
import { campo } from './ui.js';
import { avvisa, el } from './util.js';

const MAX_FOTO = 2 * 1024 * 1024;

/** Apre il foglio. Senza id crea un assistente nuovo; `dopo(id)` parte al salvataggio. */
export async function apriFoglioAssistente(id, { nomeIniziale = '', dopo, scheda: schedaIniziale = 'generale', apriCassaforte } = {}) {
  await caricaPersonaggi();
  const nuovo = !id;
  const a = nuovo ? { nome: nomeIniziale } : assistenteDi(id);
  const proposto = nuovo ? personaggioPer(nomeIniziale) : null;
  const dati = {
    nome: a.nome || '', titolo: a.titolo || '', descrizione: a.descrizione || '',
    avatar_forma: proposto ? `personaggio:${proposto}` : formaDi(a), avatar_colore: coloreDi(a), avatar_url: fotoDi(a),
    provider_predefinito_id: a.provider_predefinito_id || '', modello_predefinito_id: a.modello_predefinito_id || '', livello_ragionamento: a.livello_ragionamento || '',
    macchina_predefinita_id: a.macchina_predefinita_id || '', usa_chrome: !!a.usa_chrome,
    voce_id: a.voce_id || '', voce_velocita: a.voce_velocita || 1, voce_lingua: a.voce_lingua || 'it',
    sezione_id: a.sezione_id || '__non_assegnati__', fissato: !!a.fissato, nascosto: !!a.nascosto, colore_in_chat: a.colore_in_chat !== 0,
    memoria_cartella: a.memoria_cartella || '',
  };
  let fotoNuova = null;     // File da caricare al salvataggio
  let fotoDaTogliere = false;
  let galleria = formaDi(a).startsWith('personaggio:') || proposto ? 'personaggi' : ICONE.includes(formaDi(a)) ? 'icone' : 'forme';
  let scheda = nuovo ? 'generale' : schedaIniziale;

  const anteprima = el('span.anteprima-avatar', { 'aria-hidden': 'true' });
  const testaAvatar = el('span.avatar-testa');
  const disegnaAnteprima = () => {
    const a2 = { ...dati, avatar_url: fotoDaTogliere ? null : dati.avatar_url };
    anteprima.replaceChildren(avatarSvg(a2, 56));
    testaAvatar.replaceChildren(avatarSvg(a2, 32));
  };
  let avatarScelto = !nuovo || !!proposto; // scelto a mano: il nome non lo cambia più
  const titolo = el('h2', {}, nuovo ? 'Nuovo assistente' : dati.nome);
  const errore = el('p.errore-modulo', { role: 'alert', hidden: true });
  const corpo = el('div.foglio-corpo');
  const linguette = el('div.schede-pillola.foglio-schede', { role: 'tablist' });

  // ---------- Avatar ----------
  const scelte = el('div.galleria-avatar', { role: 'group', 'aria-label': 'Avatar' });
  const colori = el('div.scelta-avatar', { role: 'group', 'aria-label': 'Colore' });
  const sceltaFile = el('input', { type: 'file', accept: 'image/png,image/jpeg,image/webp,image/svg+xml', hidden: true, onchange: (ev) => {
    const f = ev.target.files[0];
    ev.target.value = '';
    if (!f) return;
    if (f.size > MAX_FOTO) { avvisa('La foto supera 2 MB'); return; }
    fotoNuova = f;
    fotoDaTogliere = false;
    dati.avatar_url = URL.createObjectURL(f);
    disegnaAvatar();
  } });
  const disegnaAvatar = () => {
    const voci = galleria === 'forme' ? Object.keys(FORME) : galleria === 'icone' ? ICONE : [...PERSONAGGI.keys()].map((k) => `personaggio:${k}`);
    const etichetta = (k) => (galleria === 'forme' ? FORME[k].nome : galleria === 'icone' ? k : PERSONAGGI.get(k.slice(12))?.etichetta || k);
    scelte.replaceChildren(...voci.map((k) => el('button', {
      type: 'button', 'aria-label': etichetta(k), title: etichetta(k), 'aria-pressed': dati.avatar_forma === k && !dati.avatar_url ? 'true' : 'false',
      onclick: () => { dati.avatar_forma = k; avatarScelto = true; if (dati.avatar_url) { fotoDaTogliere = !fotoNuova; fotoNuova = null; dati.avatar_url = null; } disegnaAvatar(); },
    }, avatarSvg({ avatar_forma: k, avatar_colore: dati.avatar_colore }, 34, { senzaFoto: true }))));
    colori.replaceChildren(...Object.entries(COLORI).map(([k, nome]) => el('button', {
      type: 'button', 'aria-label': nome, title: nome, 'aria-pressed': dati.avatar_colore === k ? 'true' : 'false',
      onclick: () => { dati.avatar_colore = k; avatarScelto = true; disegnaAvatar(); },
    }, el('span.colore', { style: `background:${k}` }))));
    colori.hidden = galleria === 'personaggi';
    disegnaAnteprima();
  };
  const linguetteAvatar = el('div.schede-pillola.piccole', { role: 'tablist', 'aria-label': 'Galleria' });
  const disegnaLinguetteAvatar = () => linguetteAvatar.replaceChildren(...[['forme', 'Forme'], ['icone', 'Icone'], ['personaggi', 'Personaggi']].map(([k, t]) => el('button', {
    type: 'button', role: 'tab', 'aria-selected': galleria === k ? 'true' : 'false', onclick: () => { galleria = k; disegnaLinguetteAvatar(); disegnaAvatar(); },
  }, t)));

  // ---------- Generale ----------
  const legato = (etichetta, chiave, attr = {}, largo = false) => {
    const n = campo(etichetta, { ...attr, valore: dati[chiave] }, largo);
    n.querySelector('input,textarea,select').addEventListener('input', (ev) => {
      dati[chiave] = ev.target.value;
      if (chiave === 'nome') { titolo.textContent = ev.target.value || 'Nuovo assistente'; disegnaAnteprima(); }
    });
    if (chiave === 'nome') n.querySelector('input').addEventListener('blur', () => {
      const p = personaggioPer(dati.nome);
      if (!p || avatarScelto || dati.avatar_url) return;
      dati.avatar_forma = `personaggio:${p}`;
      galleria = 'personaggi';
      disegnaLinguetteAvatar();
      disegnaAvatar();
    });
    return n;
  };
  const casella = (testo, chiave) => el('label.casella', {}, el('input', { type: 'checkbox', checked: dati[chiave], onchange: (ev) => { dati[chiave] = ev.target.checked; } }), testo);

  const generale = () => [
    el('div.campo-modulo.largo', {}, 'Avatar',
      el('div.avatar-editor', {}, anteprima, el('div.avatar-tasti', {},
        el('button.tasto.piccolo', { type: 'button', onclick: () => sceltaFile.click() }, 'Carica da file'),
        dati.avatar_url ? el('button.tasto.piccolo.pericolo', { type: 'button', onclick: () => { fotoDaTogliere = !fotoNuova; fotoNuova = null; dati.avatar_url = null; disegna(); } }, 'Rimuovi foto') : null,
        el('small', {}, 'png, jpg, webp o svg fino a 2 MB, ritagliata in cerchio'))),
      linguetteAvatar, scelte, colori, sceltaFile),
    legato('Nome', 'nome', { required: true, autocomplete: 'off', maxlength: 80 }),
    legato('Titolo', 'titolo', { placeholder: 'es. Caposquadra vendite', maxlength: 120 }),
    legato('Descrizione', 'descrizione', { rows: 2, maxlength: 2000 }, true),
    legato('Sezione', 'sezione_id', { opzioni: stato.barra.map((s) => [s.id, s.nome]) }),
    legato('Cartella della memoria', 'memoria_cartella', { placeholder: 'es. Memoria/Vendite', spellcheck: 'false' }),
    el('fieldset.caselle.largo', {}, el('legend', {}, 'Mostra'), casella('Fissato in alto', 'fissato'), casella('Nascosto dalla barra', 'nascosto'), casella('Il suo colore nelle bolle', 'colore_in_chat')),
    nuovo ? null : el('div.campo-modulo.largo', {}, el('button.tasto.piccolo.pericolo', { type: 'button', onclick: archivia }, 'Archivia l\'assistente')),
  ];

  async function archivia() {
    if (!await chiediConferma({ titolo: `Archiviare ${dati.nome}?`, testo: 'Sparisce dalla barra. Le sessioni restano nel database.', conferma: 'Archivia', pericolo: true })) return;
    try { await api.archiviaAssistente(id); await Promise.all([caricaAssistenti(), caricaBarra()]); chiudi(); avvisa(`${dati.nome} archiviato`); dopo?.(null); } catch (e) { avvisa(testoErrore(e)); }
  }

  const SCHEDE = [['generale', 'Generale'], ['modello', 'Modello e voce'], ['profilo', 'Profilo'], ['abilita', 'Abilità'], ['collegamenti', 'Collegamenti'], ['segreti', 'Segreti'], ['routine', 'Routine'], ['computer', 'Computer']];
  const salvabili = new Set(['generale', 'modello']);
  const salva = el('button.tasto.primario', { type: 'submit' }, nuovo ? 'Crea' : 'Salva');
  function disegna() {
    linguette.replaceChildren(...SCHEDE.filter(([k]) => !nuovo || salvabili.has(k)).map(([k, t]) => el('button', {
      type: 'button', role: 'tab', 'aria-selected': scheda === k ? 'true' : 'false', onclick: () => { scheda = k; disegna(); },
    }, t)));
    salva.hidden = !salvabili.has(scheda);
    const contenuto = {
      generale, modello: () => schedaModelloVoce(dati), profilo: () => schedaProfilo(id), abilita: () => [sezioneAbilita(id)],
      collegamenti: () => schedaCollegamenti(id), segreti: () => [sezioneSegreti(id, (o) => { chiudi(); apriCassaforte?.(o); })], routine: () => [sezioneRoutine(id)], computer: () => schedaComputer(id),
    }[scheda]();
    corpo.classList.toggle('una', !['generale', 'modello'].includes(scheda));
    corpo.replaceChildren(...[contenuto].flat().filter(Boolean), errore);
    if (scheda === 'generale') { disegnaLinguetteAvatar(); disegnaAvatar(); }
  }

  const modulo = el('form', { style: 'display:contents', onsubmit: async (ev) => {
    ev.preventDefault();
    if (!dati.nome.trim()) { errore.textContent = 'Il nome è obbligatorio.'; errore.hidden = false; scheda = 'generale'; disegna(); return; }
    salva.disabled = true;
    errore.hidden = true;
    const c = {
      nome: dati.nome.trim(), titolo: dati.titolo.trim() || null, descrizione: dati.descrizione.trim() || null,
      avatar_forma: dati.avatar_forma, avatar_colore: dati.avatar_colore,
      provider_predefinito_id: dati.provider_predefinito_id || null, modello_predefinito_id: dati.modello_predefinito_id || null,
      macchina_predefinita_id: dati.macchina_predefinita_id || null, usa_chrome: dati.usa_chrome ? 1 : 0,
      livello_ragionamento: dati.livello_ragionamento || null, voce_id: dati.voce_id || null, voce_velocita: Number(dati.voce_velocita) || 1, voce_lingua: dati.voce_lingua || null,
      sezione_id: dati.sezione_id === '__non_assegnati__' ? null : dati.sezione_id,
      fissato: dati.fissato ? 1 : 0, nascosto: dati.nascosto ? 1 : 0, colore_in_chat: dati.colore_in_chat ? 1 : 0, memoria_cartella: dati.memoria_cartella.trim() || null,
    };
    try {
      const fatto = nuovo ? await api.creaAssistente(c) : await api.modificaAssistente(id, c);
      const aid = fatto?.id || id;
      if (fotoNuova) await api.caricaAvatar(aid, fotoNuova).catch((e) => avvisa(`Foto non caricata: ${testoErrore(e)}`, 4000));
      else if (fotoDaTogliere) await api.togliAvatar(aid).catch(() => {});
      await Promise.all([caricaAssistenti(), caricaBarra()]);
      chiudi();
      avvisa(nuovo ? `${c.nome} creato` : 'Salvato');
      dopo?.(aid);
      bus.emetti('assistente:aggiornato', aid);
    } catch (e) {
      errore.textContent = testoErrore(e);
      errore.hidden = false;
    } finally { salva.disabled = false; }
  } },
  el('div.foglio-testa', {}, testaAvatar, titolo),
  linguette, corpo,
  el('div.foglio-piede', {}, el('span.spazio'), el('button.tasto', { type: 'button', onclick: () => chiudi() }, 'Chiudi'), salva));

  const { chiudi, nodo } = apriSovrapposta(modulo, { classe: 'foglio.largo', etichetta: nuovo ? 'Nuovo assistente' : `Impostazioni di ${dati.nome}` });
  disegna();
  nodo.querySelector('input:not([type=file]):not([type=checkbox])')?.focus();
}
