-- Migrazione 004 (2026-10-24, Fase 2.D S1): biglietti per collegare un computer, indici per
-- allegati, routine e usi della cassaforte. Le abilità degli assistenti stanno nei file
-- (<profili>/<id>/abilita/<nome>.md), non in una tabella.
CREATE TABLE biglietto_macchina (            -- «collega un computer»: il biglietto vale una volta
  hash         TEXT PRIMARY KEY,             -- sha256 del biglietto, mai il biglietto
  creato_ms    INTEGER NOT NULL,
  scade_ms     INTEGER NOT NULL,
  usato_ms     INTEGER,
  macchina_id  TEXT REFERENCES macchina(id) ON DELETE SET NULL
) STRICT;
CREATE INDEX allegato_sessione ON allegato(sessione_id, caricato_ms);
CREATE INDEX routine_scadenza ON routine(abilitata, prossima_ms);
CREATE INDEX cassaforte_uso_voce ON cassaforte_uso(voce_id, ms);
