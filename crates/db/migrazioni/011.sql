-- Migrazione 011 (2026-10-25, Fase 2.L): un collegamento acceso vale per tutti i bot, salvo
-- spegnerlo su uno. `collegamento_assistente` cambia significato: una riga (abilitato = 1) vuol dire
-- «escluso». Conversione: un collegamento che era acceso per alcuni bot esclude tutti gli altri
-- (resta com'era per chi c'era); uno che non era acceso per nessuno vale per tutti (regola nuova).
CREATE TEMP TABLE prima_011 AS
  SELECT collegamento_id, assistente_id FROM collegamento_assistente WHERE abilitato = 1;
DELETE FROM collegamento_assistente;
INSERT INTO collegamento_assistente (collegamento_id, assistente_id, abilitato)
  SELECT c.collegamento_id, a.id, 1
    FROM (SELECT DISTINCT collegamento_id FROM prima_011) c
    CROSS JOIN assistente a
   WHERE NOT EXISTS (SELECT 1 FROM prima_011 p
                      WHERE p.collegamento_id = c.collegamento_id AND p.assistente_id = a.id);
DROP TABLE prima_011;
