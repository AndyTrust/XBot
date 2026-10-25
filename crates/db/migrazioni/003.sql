-- Migrazione 003 (2026-10-24, Fase 2.B-bis): il costo del turno è quello che dichiara il client,
-- in dollari (Claude Code: total_cost_usd). La colonna prende il nome giusto.
ALTER TABLE turno RENAME COLUMN costo_stimato_eur TO costo_usd;
