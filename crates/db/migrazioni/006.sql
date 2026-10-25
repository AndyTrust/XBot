-- Migrazione 006 (2026-10-24, Fase 2.E S-B): computer collegati con il lavoratore.
-- La credenziale del lavoratore (solo l'hash sha256), dove gira ogni provider (lo stesso
-- provider può stare su più macchine) e la macchina predefinita di ogni assistente.
ALTER TABLE macchina ADD COLUMN credenziale_hash TEXT;
CREATE UNIQUE INDEX macchina_credenziale ON macchina(credenziale_hash) WHERE credenziale_hash IS NOT NULL;

CREATE TABLE provider_macchina (
  provider_id  TEXT NOT NULL REFERENCES provider(id) ON DELETE CASCADE,
  macchina_id  TEXT NOT NULL REFERENCES macchina(id) ON DELETE CASCADE,
  programma    TEXT NOT NULL,             -- percorso dell'eseguibile su quella macchina
  versione     TEXT,
  visto_ms     INTEGER NOT NULL,          -- ultimo battito che lo ha dichiarato
  PRIMARY KEY (provider_id, macchina_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX provider_macchina_macchina ON provider_macchina(macchina_id);

ALTER TABLE assistente ADD COLUMN macchina_predefinita_id TEXT REFERENCES macchina(id) ON DELETE SET NULL;
-- «Usa Chrome del Mac»: il turno porta `chrome: true` e va sulla macchina predefinita.
ALTER TABLE assistente ADD COLUMN usa_chrome INTEGER NOT NULL DEFAULT 0 CHECK (usa_chrome IN (0,1));
