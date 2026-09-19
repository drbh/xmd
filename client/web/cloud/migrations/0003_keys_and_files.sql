-- API keys let the command line sync on behalf of a person, outside the
-- browser sign-in. Only a hash is stored; the key is shown once.
CREATE TABLE api_keys (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id),
  name TEXT NOT NULL,
  hash TEXT NOT NULL UNIQUE,
  created_at INTEGER NOT NULL,
  last_used_at INTEGER,
  revoked_at INTEGER
);
CREATE INDEX api_keys_user ON api_keys(user_id);
-- A document's file name is its identity for imports and on disk, unique
-- within its folder; the display name keeps following the first heading.
ALTER TABLE documents ADD COLUMN file TEXT;
