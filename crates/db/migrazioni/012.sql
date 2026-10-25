-- Migrazione 012 (2026-10-25, sessioni annidate e lavagna comune).
-- Una sessione figlia ha `sessione_padre_id`; il padre ne vede solo stato e riassunto.
ALTER TABLE sessione ADD COLUMN figlia_stato TEXT
  CHECK (figlia_stato IS NULL OR figlia_stato IN ('al_lavoro','in_attesa','finita','fallita'));
ALTER TABLE sessione ADD COLUMN figlia_riassunto TEXT;
CREATE INDEX sessione_padre ON sessione(sessione_padre_id) WHERE sessione_padre_id IS NOT NULL;

-- La lavagna: chi fa cosa, su quali file, da quando. Una sola fonte per tutti gli agenti.
-- `origine` = 'xbot' (scritta da un assistente) o 'registro' (importata da lavori.py).
CREATE TABLE lavagna (
  id             TEXT PRIMARY KEY,
  assistente_id  TEXT REFERENCES assistente(id) ON DELETE SET NULL,
  sessione_id    TEXT REFERENCES sessione(id) ON DELETE SET NULL,
  agente         TEXT NOT NULL,              -- nome mostrato (menzione dell'assistente o agente del registro)
  macchina_id    TEXT,
  progetto       TEXT NOT NULL DEFAULT '',
  cosa           TEXT NOT NULL,
  file_json      TEXT NOT NULL DEFAULT '[]',
  risorse_json   TEXT NOT NULL DEFAULT '[]',
  origine        TEXT NOT NULL DEFAULT 'xbot' CHECK (origine IN ('xbot','registro')),
  chiave_esterna TEXT UNIQUE,                -- nome del file del registro, per non importare due volte
  stato          TEXT NOT NULL DEFAULT 'attivo' CHECK (stato IN ('attivo','finito')),
  inizio_ms      INTEGER NOT NULL,
  battito_ms     INTEGER NOT NULL,
  fine_ms        INTEGER,
  esito          TEXT
) STRICT;
CREATE INDEX lavagna_attive ON lavagna(stato, battito_ms DESC);
