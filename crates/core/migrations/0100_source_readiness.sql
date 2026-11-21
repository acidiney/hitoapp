CREATE TABLE source_readiness_jobs (
    user_id TEXT PRIMARY KEY REFERENCES users(id),
    cursor TEXT CHECK(cursor IS NULL OR length(cursor) <= 128),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts >= 0),
    lease_token TEXT,
    lease_until INTEGER,
    next_run_at INTEGER NOT NULL DEFAULT 0,
    requested_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK((lease_token IS NULL) = (lease_until IS NULL))
);
CREATE INDEX source_readiness_jobs_due ON source_readiness_jobs(next_run_at);
CREATE TABLE source_readiness_checks (
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    checked_at INTEGER NOT NULL,
    PRIMARY KEY(user_id, recording_id)
);
CREATE INDEX source_readiness_check_recording ON source_readiness_checks(recording_id);
CREATE TABLE account_source_readiness_preferences (
    user_id TEXT PRIMARY KEY REFERENCES users(id),
    enabled INTEGER NOT NULL CHECK(enabled IN (0,1))
);
