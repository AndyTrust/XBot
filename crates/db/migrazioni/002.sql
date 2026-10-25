-- Migrazione 002 (2026-10-24, Fase 2.B): i non letti della barra per tempo, non per seq.
-- riga.seq è per sessione, letto_fino_seq era per assistente: con più sessioni il conto
-- usciva sbagliato. Ora conta i messaggi dell'assistente creati dopo l'ultima vista.
DROP VIEW IF EXISTS v_barra_laterale;
CREATE VIEW v_barra_laterale AS               -- quello che la barra laterale mostra
SELECT sb.id AS sezione_id, sb.nome AS sezione, sb.ordine AS sezione_ordine, sb.chiusa,
       a.id AS assistente_id, a.nome, a.avatar_forma, a.avatar_colore, a.fissato,
       COALESCE(sl.in_attesa_utente, 0) AS in_attesa_utente,
       COALESCE(sl.al_lavoro, 0) AS al_lavoro,
       (SELECT COUNT(*) FROM riga r JOIN sessione s ON s.id = r.sessione_id
         WHERE s.assistente_id = a.id AND r.creata_ms > COALESCE(sl.ultima_vista_ms, 0)
           AND r.ruolo = 'assistente' AND r.genere = 'messaggio' AND r.soppressa = 0) AS non_letti
FROM assistente a
JOIN sezione_assistente sa ON sa.assistente_id = a.id
JOIN sezione_barra sb ON sb.id = sa.sezione_id
LEFT JOIN stato_lettura sl ON sl.assistente_id = a.id
WHERE a.archiviato_ms IS NULL AND a.nascosto = 0;
