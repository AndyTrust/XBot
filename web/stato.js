// Lo stato condiviso dell'interfaccia e i caricamenti dal server.

import { api } from './api.js';
import { creaBus, locale } from './util.js';

export const bus = creaBus();

export const stato = {
  io: null,              // GET /io
  barra: [],             // GET /barra: sezioni con assistenti
  assistenti: new Map(), // id → assistente completo (GET /assistenti)
  provider: [],          // GET /provider
  modelli: new Map(),    // provider_id → [modelli]
  quote: [],             // GET /quote
  impostazioni: {},      // GET /impostazioni (mappa piatta)
  assistenteId: null,    // l'assistente aperto
  sessione: null,        // la sessione aperta
  sessioni: [],          // le sessioni dell'assistente aperto
  turno: null,           // {turno_id, stato} del turno in corso
  pannello: locale.leggi('pannello', true),
};

// Chiavi delle impostazioni dell'interfaccia (tabella `impostazione`).
export const CHIAVI = {
  tema: 'interfaccia.tema',           // chiaro | scuro | sistema
  accento: 'interfaccia.accento',     // assistente | neutro
  lingua: 'interfaccia.lingua',       // it
  suono: 'interfaccia.suono_notifiche', // true | false
  pannello: 'interfaccia.pannello',   // true | false
  flusso: 'interfaccia.flusso_strumenti', // completo | minimo (2.J)
  ortografia: 'interfaccia.ortografia', // true | false (2.J)
};

export function impostazione(chiave, predefinito) {
  const v = stato.impostazioni[chiave];
  return v === undefined || v === null ? predefinito : v;
}

export async function scriviImpostazione(chiave, valore) {
  stato.impostazioni[chiave] = valore;
  applicaAspetto();
  bus.emetti('impostazioni');
  const nuove = await api.scriviImpostazioni({ [chiave]: valore });
  if (nuove && typeof nuove === 'object') stato.impostazioni = nuove;
  locale.scrivi('aspetto', { tema: impostazione(CHIAVI.tema, 'sistema'), accento: impostazione(CHIAVI.accento, 'assistente') });
}

/** Tema e accento sul documento. Prima dell'accesso vale l'ultima scelta salvata nel browser. */
export function applicaAspetto() {
  const salvato = locale.leggi('aspetto', {});
  const tema = impostazione(CHIAVI.tema, salvato.tema || 'sistema');
  document.documentElement.dataset.tema = ['chiaro', 'scuro', 'sistema'].includes(tema) ? tema : 'sistema';
  document.documentElement.dataset.accento = impostazione(CHIAVI.accento, salvato.accento || 'assistente');
}

export async function caricaBarra() {
  const b = await api.barra();
  // «Non assegnati» sempre in fondo.
  b.sort((x, y) => (x.id === '__non_assegnati__') - (y.id === '__non_assegnati__') || x.ordine - y.ordine);
  stato.barra = b;
  bus.emetti('barra');
  return b;
}

export async function caricaAssistenti() {
  const elenco = await api.assistenti();
  stato.assistenti = new Map(elenco.map((a) => [a.id, a]));
  bus.emetti('assistenti');
  return elenco;
}

/** Fonde le voci della barra (stato in tempo reale) con l'assistente completo. */
export function assistenteDi(id) {
  const pieno = stato.assistenti.get(id) || {};
  let voce = null;
  for (const s of stato.barra) for (const v of s.assistenti || []) if (v.assistente_id === id) voce = v;
  return { ...pieno, ...(voce || {}), id };
}

export async function caricaQuote() {
  try { stato.quote = await api.quote(); } catch { stato.quote = []; }
  bus.emetti('quote');
  return stato.quote;
}

export function quotaDi(providerId) {
  const q = stato.quote.find((x) => x.provider_id === providerId);
  if (!q) return null;
  let dettaglio = q.dettaglio_json;
  if (typeof dettaglio === 'string') { try { dettaglio = JSON.parse(dettaglio); } catch { dettaglio = null; } }
  return { ...q, dettaglio };
}

export async function caricaProvider() {
  stato.provider = await api.provider();
  bus.emetti('provider');
  return stato.provider;
}

export async function modelliDi(providerId) {
  if (!providerId) return [];
  if (!stato.modelli.has(providerId)) stato.modelli.set(providerId, await api.modelli(providerId));
  return stato.modelli.get(providerId);
}

/** Cosa sceglie il server con «Auto»: claude_code se è attivo, altrimenti il primo attivo. */
export function providerAuto() {
  const attivi = stato.provider.filter((p) => p.attivo && p.caricato !== false);
  return (attivi.find((p) => p.id === 'claude_code') || attivi[0])?.id || null;
}

/** 2.L: un provider con `attivo = 0` (Codex) è spento e non compare all'utente; uno sconosciuto si mostra. */
export function providerAcceso(id) {
  const p = stato.provider.find((x) => x.id === id);
  return !p || (p.attivo !== 0 && p.attivo !== false);
}

/** I provider accesi, nell'ordine del server. */
export const providerAccesi = (elenco = stato.provider) => elenco.filter((p) => p.attivo !== 0 && p.attivo !== false && providerAcceso(p.id));

export function nomeProvider(id) {
  return stato.provider.find((p) => p.id === id)?.nome || id || 'Auto';
}

/** Chiave della bozza di una sessione nella memoria del browser. */
export const chiaveBozza = (sessioneId) => `bozza.${sessioneId}`;
