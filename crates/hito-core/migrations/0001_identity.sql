CREATE TABLE users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    admin INTEGER NOT NULL DEFAULT 0 CHECK (admin IN (0,1)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL
);
CREATE INDEX sessions_user ON sessions(user_id);
CREATE TABLE libraries (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL
);
CREATE TABLE library_members (
    library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (library_id, user_id)
);
