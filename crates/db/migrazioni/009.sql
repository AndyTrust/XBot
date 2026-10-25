-- Migrazione 009 (2026-10-25, Fase 2.H): il computer di ogni assistente. Con il computer acceso
-- il server tiene sulla VPS un desktop suo (schermo virtuale, Chrome col suo profilo, terminale),
-- visto dalla scheda Computer. `computer_acceso` è la volontà dell'utente (all'avvio i computer accesi
-- ripartono); `computer_assistente` è lo stato vero dei processi, con le porte (solo 127.0.0.1).
ALTER TABLE assistente ADD COLUMN computer_acceso INTEGER NOT NULL DEFAULT 0
  CHECK (computer_acceso IN (0,1));
CREATE TABLE computer_assistente (
  assistente_id  TEXT PRIMARY KEY REFERENCES assistente(id) ON DELETE CASCADE,
  stato          TEXT NOT NULL CHECK (stato IN ('acceso','avvio','spento','errore')),
  display        INTEGER,                   -- :N di Xvfb (50 + posto)
  porta_vnc      INTEGER,                   -- x11vnc (5950 + posto)
  porta_ws       INTEGER,                   -- riservata (6950 + posto): lo schermo passa dal server
  porta_ttyd     INTEGER,                   -- ttyd (7950 + posto)
  porta_chrome   INTEGER,                   -- Chrome --remote-debugging-port (9350 + posto)
  larghezza      INTEGER NOT NULL DEFAULT 1280,
  altezza        INTEGER NOT NULL DEFAULT 800,
  acceso_ms      INTEGER,
  errore         TEXT,
  aggiornato_ms  INTEGER NOT NULL
) STRICT;
