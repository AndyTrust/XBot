-- Migrazione 005 (2026-10-24, Fase 2.D-bis S2): tetto orario degli invii di un canale
-- (`invia_a_assistente`), modificabile con PATCH /canali/{id}.
ALTER TABLE canale ADD COLUMN limite_orario INTEGER NOT NULL DEFAULT 30 CHECK (limite_orario BETWEEN 0 AND 1000);
