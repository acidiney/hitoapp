CREATE TABLE correction_suggestions (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    correction_id TEXT UNIQUE NOT NULL,
    reporter_id TEXT NOT NULL REFERENCES reporters(id) ON DELETE CASCADE,
    submission_id TEXT NOT NULL,
    left_provider TEXT NOT NULL CHECK (left_provider IN ('youtube', 'jamendo')),
    left_recording_id TEXT NOT NULL,
    right_provider TEXT NOT NULL CHECK (right_provider IN ('youtube', 'jamendo')),
    right_recording_id TEXT NOT NULL,
    relation TEXT NOT NULL CHECK (relation = 'same_recording'),
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'rejected')),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision BETWEEN 1 AND 9007199254740991),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (reporter_id, submission_id),
    CHECK (left_provider < right_provider OR (left_provider = right_provider AND left_recording_id < right_recording_id))
);

CREATE INDEX corrections_reporter_created ON correction_suggestions (reporter_id, created_at);
CREATE INDEX corrections_status_sequence ON correction_suggestions (status, sequence);
CREATE INDEX corrections_reporter_status_sequence ON correction_suggestions (reporter_id, status, sequence);

CREATE TABLE correction_review_receipts (
    request_id TEXT PRIMARY KEY NOT NULL,
    correction_id TEXT NOT NULL,
    expected_revision INTEGER NOT NULL CHECK (expected_revision BETWEEN 1 AND 9007199254740990),
    result_revision INTEGER NOT NULL CHECK (result_revision = expected_revision + 1),
    decision TEXT NOT NULL CHECK (decision IN ('approve', 'reject')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (correction_id, result_revision)
);
