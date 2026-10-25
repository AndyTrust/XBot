// Il pannello di destra (Layout sez. 1.4): Dettagli · Libreria · Computer.
// Dettagli: avatar grande, nome, titolo; riquadro «ora» con il bottone «macchina · provider ·
// modello» che apre il foglio di scelta, «Pensiero» e chip della macchina con stato; ROUTINE, MEMORIA, ABILITÀ, FILE, SEGRETI; Impostazioni dell'assistente.

import { avatarVivo } from './avatar.js';
import { sezioneLavagna } from './figlie.js';
import { icona } from './icone.js';
import { sezioneAbilita } from './pagine/abilita.js';
import { sezioneRoutine } from './pagine/routine.js';
import { sezioneComputer, sezioneFile, sezioneLibreria, sezioneMemoria, sezioneSegreti } from './pannello-schede.js';
import { etichettaMacchina, inLinea, STATI_MACCHINA } from './macchine.js';
import { continuaSu, macchinaDellaSessione, pensieroSessione, sceltaAttuale, sceltaGruppata, tastoQuota } from './scrittura.js';
import { assistenteDi, bus, providerAuto, stato } from './stato.js';
import { chiudiVista, puntoComputer, sezioneSchermo, vistaAPieno } from './schermo.js';
import { coppia } from './ui.js';
import { $, el, locale } from './util.js';

let scheda = 'dettagli';
let azioni = {};
let ultimo = null; // assistente e scheda dell'ultimo disegno: si ricarica solo se cambiano

export function preparaPannello(a) {
  azioni = a; // {chiudi, impostazioniAssistente, apriCassaforte}
  for (const e of ['quote', 'provider', 'assistenti', 'macchine']) bus.su(e, () => { if (!$('#pannello').hidden) disegnaOra(); });
  // sessione cambiata (Continua su…, provider, modello): la scheda Computer si ridisegna intera
  bus.su('sessione', () => { if ($('#pannello').hidden) return; if (scheda === 'computer') disegnaPannello(); else disegnaOra(); });
  // salvataggi del foglio dell'assistente, routine e abilità nuove: si rileggono le sezioni
  bus.su('assistente:aggiornato', () => { if (!$('#pannello').hidden) disegnaPannello(); });
  // 2.H: pallino del computer sulla linguetta; pannello chiuso → si chiude la vista (non il computer)
  bus.su('computer', ({ aid, stato: st }) => { const t = $('#linguetta-computer'); if (t && aid === stato.assistenteId) t.querySelector('.punto-computer')?.replaceWith(puntoComputer(st)); });
  new MutationObserver(() => { if ($('#pannello').hidden && !vistaAPieno()) chiudiVista(); }).observe($('#pannello'), { attributes: true, attributeFilter: ['hidden'] });
}

/** Ridisegna il pannello intero (cambio di assistente o di scheda). */
export function disegnaPannello() {
  const p = $('#pannello');
  if (!stato.assistenteId) { chiudiVista(); p.replaceChildren(); return; }
  if (scheda !== 'computer') chiudiVista();
  const a = assistenteDi(stato.assistenteId);
  ultimo = `${a.id}:${scheda}`;
  const schede = [['dettagli', 'Dettagli'], ['libreria', 'Libreria'], ['computer', 'Computer']];
  const corpo = el('div.scheda-corpo', { role: 'tabpanel' });
  if (scheda === 'dettagli') {
    corpo.append(
      el('div.riquadro-titolo', {}, 'Ora'),
      el('div.riquadro', { id: 'pannello-ora' }),
      ...sezioneLavagna(),
      sezioneRoutine(a.id, { compatta: true }),
      sezioneMemoria(a.id),
      sezioneAbilita(a.id, { compatta: true }),
      sezioneFile(a.id, () => { scheda = 'libreria'; disegnaPannello(); }),
      sezioneSegreti(a.id, azioni.apriCassaforte),
      el('button.tasto', { type: 'button', onclick: () => azioni.impostazioniAssistente(stato.assistenteId) }, icona('impostazioni', 14), 'Impostazioni dell\'assistente'),
    );
  } else if (scheda === 'libreria') {
    corpo.append(sezioneLibreria(a.id));
  } else {
    const torna = () => { scheda = 'dettagli'; locale.scrivi('scheda_pannello', scheda); disegnaPannello(); };
    corpo.append(el('div.riquadro-titolo', {}, 'Computer dell\'assistente'), sezioneSchermo(a.id, torna), sezioneComputer(() => stato.sessione, (id) => continuaSu(id)));
  }
  p.replaceChildren(
    el('div.pannello-testa', {}, el('button.tasto-icona', { type: 'button', 'aria-label': 'Chiudi il pannello', onclick: () => azioni.chiudi() }, icona('chiudi', 16))),
    el('div.pannello-chi', {}, avatarVivo(a, 88), el('h2', {}, a.nome || ''),
      el('button.titolo', { type: 'button', onclick: () => azioni.impostazioniAssistente(a.id) }, a.titolo || 'Aggiungi un\'etichetta')),
    el('div.schede', { role: 'tablist' }, schede.map(([id, t]) => el('button', {
      type: 'button', role: 'tab', 'aria-selected': scheda === id ? 'true' : 'false',
      id: `linguetta-${id}`, onclick: () => { scheda = id; locale.scrivi('scheda_pannello', id); disegnaPannello(); },
    }, t, id === 'computer' ? puntoComputer(a.computer?.stato) : null))),
    corpo,
  );
  disegnaOra();
}

/** Ridisegna solo il riquadro «ora» (provider, modello, quota, macchina). */
export async function disegnaOra() {
  const box = $('#pannello-ora');
  if (!box || !stato.assistenteId) return;
  if (ultimo !== `${stato.assistenteId}:${scheda}`) { disegnaPannello(); return; }
  const a = assistenteDi(stato.assistenteId);
  const s = stato.sessione;
  const auto = providerAuto();
  const providerId = s?.provider_id || a.provider_predefinito_id || auto;
  const macchina = macchinaDellaSessione(s);
  const sa = s ? await sceltaAttuale(s) : null;
  const scelta = s ? await sceltaGruppata(s, { classe: 'span.scelta-pannello' }) : el('span', {}, '—');
  const pensiero = s ? await pensieroSessione(s, sa) : null;
  if (box !== $('#pannello-ora')) return;
  const su = !macchina || inLinea(macchina);
  box.replaceChildren(
    coppia('Con cosa', scelta, 'Macchina · provider · modello'),
    pensiero ? coppia('Pensiero', pensiero) : null,
    coppia('Quota', providerId ? tastoQuota(providerId, { lungo: false }) : '—'),
    coppia('Macchina', el('span.chip-macchina.fermo', {}, el(`span.punto-${su ? 'verde' : 'grigio'}`), el('span', {}, macchina ? etichettaMacchina(macchina) : (s?.macchina_id || 'server')),
      macchina ? el('small', {}, (STATI_MACCHINA[macchina.stato] || macchina.stato).toLowerCase()) : null)),
    a.descrizione ? coppia('Descrizione', a.descrizione) : null,
  );
}

scheda = locale.leggi('scheda_pannello', 'dettagli');
