ALTER TABLE sessions ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN last_seen_at INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN client TEXT CHECK (client IS NULL OR length(CAST(client AS BLOB)) BETWEEN 1 AND 64);
UPDATE sessions SET created_at = expires_at - 2592000, last_seen_at = expires_at - 2592000;
