CREATE TABLE playing_recordings (
    session_hash TEXT NOT NULL REFERENCES sessions(token_hash) ON DELETE CASCADE,
    client TEXT NOT NULL CHECK(length(client) <= 64),
    account_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    recording_id TEXT NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    announced_at INTEGER NOT NULL DEFAULT (unixepoch()),
    next_attempt_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(session_hash,client)
);
CREATE INDEX playing_recordings_account ON playing_recordings(account_id,recording_id);

CREATE TABLE played_copy_requests (
    account_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    recording_id TEXT NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    played_at INTEGER NOT NULL DEFAULT (unixepoch()),
    next_attempt_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(account_id,recording_id)
);
CREATE INDEX played_copy_requests_recent ON played_copy_requests(played_at);
