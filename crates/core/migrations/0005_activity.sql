CREATE TABLE likes (
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    liked INTEGER NOT NULL CHECK(liked IN (0,1)),
    PRIMARY KEY(user_id,recording_id)
);
CREATE TABLE listening_progress (
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    position_ms INTEGER NOT NULL CHECK(position_ms >= 0),
    speed REAL NOT NULL DEFAULT 1,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(user_id,recording_id)
);
CREATE INDEX listening_recent ON listening_progress(user_id,updated_at DESC);
