// Il flusso SSE di una sessione e il diario degli invii idempotenti.
//
// Flusso: EventSource su /sessioni/{id}/flusso dal cursore (dopo_aggiornata_seq, generazione).
// rows → righe nuove o riscritte; cursor_too_old e cleared → si ricarica da zero;
// turno → stato del turno; heartbeat → niente. Riconnessione a mano con attesa crescente,
// sempre dal cursore aggiornato (l'auto-riconnessione di EventSource userebbe l'URL vecchio).
//
// Diario: ogni invio ha un ULID del client e resta in localStorage finché il server risponde.
// Se la rete cade si riprova; il server riconosce l'id e non duplica.

import { api, ErroreRete, testoErrore } from './api.js';
import { bus } from './stato.js';
import { avvisa, locale, ulid } from './util.js';

const ATTESE = [1000, 2000, 4000, 8000, 15000, 30000];

class Flusso {
  constructor() {
    this.es = null;
    this.sessioneId = null;
    this.dopo = 0;
    this.generazione = null;
    this.tentativo = 0;
    this.timer = null;
  }

  /** Apre il flusso di una sessione da zero. */
  apri(sessioneId) {
    this.chiudi();
    this.sessioneId = sessioneId;
    this.dopo = 0;
    this.generazione = null;
    this.tentativo = 0;
    this.connetti();
  }

  chiudi() {
    clearTimeout(this.timer);
    this.timer = null;
    if (this.es) { this.es.close(); this.es = null; }
    this.sessioneId = null;
  }

  connetti() {
    const id = this.sessioneId;
    if (!id || document.hidden) return; // 2.L difetto 11: a pagina nascosta niente richieste (riprende a `visibilitychange`)
    if (this.es) this.es.close();
    const es = new EventSource(api.urlFlusso(id, this.dopo, this.generazione ?? undefined), { withCredentials: true });
    this.es = es;
    const mio = (f) => (ev) => { if (this.es === es && this.sessioneId === id) f(ev); };
    es.addEventListener('connected', mio(() => { this.tentativo = 0; bus.emetti('flusso:stato', 'connesso'); }));
    es.addEventListener('rows', mio((ev) => {
      const d = JSON.parse(ev.data);
      if (this.generazione !== null && d.generazione !== this.generazione) {
        this.generazione = d.generazione;
        this.dopo = 0;
        bus.emetti('flusso:azzera', { sessioneId: id });
      }
      this.generazione = d.generazione;
      for (const r of d.righe) if (r.aggiornata_seq > this.dopo) this.dopo = r.aggiornata_seq;
      bus.emetti('flusso:righe', { sessioneId: id, righe: d.righe });
    }));
    const ricarica = mio((ev) => {
      let g = null;
      try { const d = JSON.parse(ev.data); g = d.nuova_generazione ?? d.generazione ?? null; } catch { /* niente */ }
      this.generazione = g;
      this.dopo = 0;
      bus.emetti('flusso:azzera', { sessioneId: id });
      if (ev.type === 'cursor_too_old') { // il server chiude dopo questo frame
        es.close();
        this.es = null;
        this.riconnettiSubito();
      }
    });
    es.addEventListener('cursor_too_old', ricarica);
    es.addEventListener('cleared', ricarica);
    es.addEventListener('turno', mio((ev) => bus.emetti('flusso:turno', { sessioneId: id, ...JSON.parse(ev.data) })));
    // dal commit e6b8268: una richiesta nasce o cambia stato; la barra va riletta
    es.addEventListener('richiesta', mio((ev) => { let d = {}; try { d = JSON.parse(ev.data); } catch { /* vuoto */ } bus.emetti('flusso:richiesta', { sessioneId: id, ...d }); }));
    es.addEventListener('barra', mio(() => bus.emetti('flusso:barra', { sessioneId: id })));
    es.addEventListener('heartbeat', () => {});
    es.onerror = mio(() => {
      // chiusura del server (vita finita) o rete: si riparte dal cursore
      es.close();
      this.es = null;
      bus.emetti('flusso:stato', 'scollegato');
      const attesa = ATTESE[Math.min(this.tentativo, ATTESE.length - 1)];
      this.tentativo++;
      clearTimeout(this.timer);
      if (!document.hidden) this.timer = setTimeout(() => this.connetti(), attesa);
    });
  }

  /** Pagina nascosta: si chiude l'EventSource (il cursore resta) e nessun tentativo è in coda. */
  sospendi() {
    clearTimeout(this.timer);
    this.timer = null;
    if (this.es) { this.es.close(); this.es = null; bus.emetti('flusso:stato', 'sospeso'); }
  }

  riconnettiSubito() {
    clearTimeout(this.timer);
    this.timer = setTimeout(() => this.connetti(), 50);
  }
}

export const flusso = new Flusso();

// 2.L difetto 11: a pagina nascosta (schermo spento) il flusso si ferma; quando torna visibile o la rete
// torna si riaggancia subito dal cursore e il resto dell'app ricarica (evento `pagina:visibile`).
addEventListener('online', () => { if (flusso.sessioneId && !flusso.es && !document.hidden) flusso.riconnettiSubito(); riprovaTutti(); });
document.addEventListener('visibilitychange', () => {
  if (document.hidden) { flusso.sospendi(); return; }
  flusso.tentativo = 0;
  if (flusso.sessioneId && !flusso.es) flusso.riconnettiSubito();
  riprovaTutti();
  bus.emetti('pagina:visibile');
});

// ---------- Diario degli invii ----------
const DIARIO = 'diario_invii';
const inVolo = new Set();
const timerRiprova = new Map();

export function diario() { return locale.leggi(DIARIO, []); }
function salvaDiario(v) { locale.scrivi(DIARIO, v.length ? v : null); }
function togli(id) { salvaDiario(diario().filter((x) => x.id !== id)); }

/** Gli invii in attesa di una sessione (per le bolle «in invio»). */
export function inAttesa(sessioneId) { return diario().filter((x) => x.sessione_id === sessioneId); }

/** Mette un invio nel diario e lo manda. */
export function invia(sessioneId, testo, extra = {}) {
  const voce = { id: ulid(), sessione_id: sessioneId, testo, composto_ms: Date.now(), tentativi: 0, ...extra };
  salvaDiario([...diario(), voce]);
  bus.emetti('diario', { sessioneId });
  manda(voce);
  return voce;
}

async function manda(voce) {
  if (inVolo.has(voce.id)) return;
  inVolo.add(voce.id);
  try {
    const esito = await api.invia(voce.sessione_id, {
      id: voce.id, testo: voce.testo, origine: voce.origine || 'pwa', composto_ms: voce.composto_ms,
      provider: voce.provider || undefined, risposta_a: voce.risposta_a || undefined,
      allegati: voce.allegati?.length ? voce.allegati.map((x) => x.id) : undefined,
    });
    togli(voce.id);
    bus.emetti('invio:esito', { voce, esito });
    if (esito?.rifiuto) avvisa(`Non inviato: ${esito.rifiuto.testo || esito.rifiuto.codice}`, 4000);
  } catch (e) {
    if (e instanceof ErroreRete || (e.stato && e.stato >= 500)) {
      // si riprova con attesa crescente: stesso id, il server non duplica
      const v = diario().find((x) => x.id === voce.id);
      if (v) {
        v.tentativi = (v.tentativi || 0) + 1;
        salvaDiario(diario().map((x) => (x.id === v.id ? v : x)));
        const attesa = ATTESE[Math.min(v.tentativi - 1, ATTESE.length - 1)];
        clearTimeout(timerRiprova.get(v.id));
        timerRiprova.set(v.id, setTimeout(() => manda(v), attesa));
      }
      bus.emetti('diario', { sessioneId: voce.sessione_id });
    } else {
      // rifiuto definitivo (4xx): il testo torna nella bozza, niente si perde
      togli(voce.id);
      bus.emetti('invio:rifiutato', { voce, errore: e });
      avvisa(`Messaggio non consegnato: ${testoErrore(e)}`, 4500);
    }
  } finally {
    inVolo.delete(voce.id);
  }
}

/** Riprova tutti gli invii rimasti nel diario (all'avvio, al ritorno della rete). */
export function riprovaTutti() {
  for (const v of diario()) manda(v);
}
