-- Migrazione 008 (2026-10-25, accesso fidato): i dispositivi fidati. Dopo un accesso completo
-- (password + TOTP) con `fidato: true` il server rilascia il cookie firmato `xbot_fidato`;
-- con quel cookie si rientra con la sola password. Si salva l'hash sha256 del valore, mai il valore.
CREATE TABLE dispositivo_fidato (
  id             TEXT PRIMARY KEY,          -- ULID, quello che vede l'interfaccia
  nome           TEXT NOT NULL,             -- il `dispositivo` dichiarato all'accesso
  hash           TEXT NOT NULL UNIQUE,      -- sha256 esadecimale del valore del cookie
  creato_ms      INTEGER NOT NULL,
  ultimo_uso_ms  INTEGER NOT NULL,
  scade_ms       INTEGER NOT NULL,
  revocato_ms    INTEGER,
  agente         TEXT                       -- User-Agent della richiesta che l'ha creato
) STRICT;
CREATE INDEX dispositivo_fidato_validi ON dispositivo_fidato(scade_ms) WHERE revocato_ms IS NULL;
