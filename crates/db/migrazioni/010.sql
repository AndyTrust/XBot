-- Migrazione 010 (2026-10-25, Fase 2.I): OAuth dei collegamenti e account dei provider.
-- `oauth_account` si rifà (era vuota e nessun codice la usava): un solo account per collegamento,
-- token cifrati con la chiave della cassaforte e cancellati (NULL) quando l'utente scollega.
DROP TABLE IF EXISTS oauth_account;
CREATE TABLE oauth_account (
  id                     TEXT PRIMARY KEY,
  collegamento_id        TEXT NOT NULL UNIQUE REFERENCES collegamento(id) ON DELETE CASCADE,
  account                TEXT NOT NULL,             -- email o nome dell'account, o l'host del server
  token_cifrato          BLOB,                      -- JSON {access_token, refresh_token?, token_type, scope?, expires_at_ms?}
  scade_ms               INTEGER,
  aggiornato_ms          INTEGER NOT NULL,
  client_id              TEXT,
  client_secret_cifrato  BLOB,
  issuer                 TEXT,
  scopi                  TEXT,
  stato                  TEXT NOT NULL DEFAULT 'ok' CHECK (stato IN ('ok','scaduto','revocato'))
) STRICT;

-- Il collegamento sa se vuole un accesso OAuth (dal catalogo o dalla scoperta) e tiene in chiaro
-- solo i nomi delle intestazioni e delle variabili (i valori restano cifrati).
ALTER TABLE collegamento ADD COLUMN richiede_oauth INTEGER NOT NULL DEFAULT 0
  CHECK (richiede_oauth IN (0,1));
ALTER TABLE collegamento ADD COLUMN intestazioni_nomi_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE collegamento ADD COLUMN ambiente_nomi_json TEXT NOT NULL DEFAULT '[]';

-- L'account del client su ogni computer, dal battito del lavoratore: JSON {nome, dettaglio}.
ALTER TABLE provider_macchina ADD COLUMN account_json TEXT;
