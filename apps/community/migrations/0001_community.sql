CREATE TABLE reporters (
    id TEXT PRIMARY KEY NOT NULL,
    token_hash BLOB UNIQUE NOT NULL CHECK (length(token_hash) = 32),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE outcomes (
    reporter_id TEXT NOT NULL REFERENCES reporters(id) ON DELETE CASCADE,
    submission_id TEXT NOT NULL,
    receipt_id TEXT UNIQUE NOT NULL,
    provider TEXT NOT NULL CHECK (provider IN ('youtube', 'jamendo')),
    recording_id TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (reporter_id, submission_id)
);

CREATE INDEX outcomes_ranking ON outcomes (provider, recording_id, reporter_id);
