CREATE TABLE playlists (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    name TEXT NOT NULL,
    deleted INTEGER NOT NULL DEFAULT 0 CHECK(deleted IN (0,1)),
    kind TEXT NOT NULL DEFAULT 'local' CHECK(kind IN ('local','followed'))
);
CREATE INDEX playlists_user ON playlists(user_id,deleted,name);
CREATE TABLE sync_operations (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL REFERENCES users(id),
    device_id TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    payload TEXT NOT NULL,
    UNIQUE(user_id,operation_id)
);
CREATE INDEX sync_user_sequence ON sync_operations(user_id,sequence);
