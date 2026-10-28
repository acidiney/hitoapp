CREATE TABLE account_pin_locks (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    pin_hash TEXT NOT NULL,
    idle_minutes INTEGER NOT NULL DEFAULT 0 CHECK (idle_minutes IN (0,5,15,60)),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TABLE session_locks (
    token_hash TEXT PRIMARY KEY REFERENCES sessions(token_hash) ON DELETE CASCADE,
    locked INTEGER NOT NULL DEFAULT 0 CHECK (locked IN (0,1)),
    failures INTEGER NOT NULL DEFAULT 0 CHECK (failures >= 0),
    active_at INTEGER NOT NULL DEFAULT (unixepoch())
);
