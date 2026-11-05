CREATE TABLE account_avatars (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    content_hash TEXT NOT NULL CHECK(length(content_hash) = 64),
    bytes BLOB NOT NULL CHECK(length(bytes) BETWEEN 1 AND 262144),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);
