CREATE TABLE listening_history (
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    played_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(user_id,recording_id)
);
CREATE INDEX listening_history_recent ON listening_history(user_id,played_at DESC);
CREATE TABLE account_history_preferences (
    user_id TEXT PRIMARY KEY REFERENCES users(id),
    history_enabled INTEGER NOT NULL CHECK(history_enabled IN (0,1))
);
INSERT INTO listening_history(user_id,recording_id,played_at)
    SELECT user_id,recording_id,updated_at FROM listening_progress;
