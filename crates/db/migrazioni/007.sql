-- Migrazione 007 (2026-10-24, Fase 2.E-bis S-B): le risposte `risolta` per un computer
-- scollegato. Se il socket è chiuso quando l'utente risponde a una richiesta del lavoratore, il
-- frame si mette qui e parte al `ciao` della riconnessione, dopo i `lavoro` aperti.
-- `consegnata_ms` si scrive nello stesso UPDATE che la prende: parte una volta sola.
CREATE TABLE risolta_in_coda (
  richiesta_id   TEXT PRIMARY KEY,          -- l'id scelto dal lavoratore
  macchina_id    TEXT NOT NULL REFERENCES macchina(id) ON DELETE CASCADE,
  lavoro_id      TEXT,
  frame_json     TEXT NOT NULL,             -- il frame `risolta` intero (nessun segreto)
  creata_ms      INTEGER NOT NULL,
  consegnata_ms  INTEGER
) STRICT;
CREATE INDEX risolta_da_consegnare ON risolta_in_coda(macchina_id, creata_ms)
  WHERE consegnata_ms IS NULL;
