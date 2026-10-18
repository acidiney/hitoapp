CREATE TABLE community_reporters (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    service_key TEXT NOT NULL CHECK(length(service_key) = 64),
    reporter_id TEXT NOT NULL CHECK(length(reporter_id) = 36),
    credential BLOB NOT NULL CHECK(length(credential) = 137),
    PRIMARY KEY(user_id,service_key)
);
CREATE INDEX community_credential_fingerprints ON community_reporters(substr(credential,1,33));

CREATE TABLE community_reports (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    submission_id TEXT NOT NULL CHECK(length(submission_id) = 36),
    service_key TEXT NOT NULL CHECK(length(service_key) = 64),
    session_hash TEXT NOT NULL,
    source_id TEXT NOT NULL REFERENCES sources(id),
    provider TEXT NOT NULL CHECK(provider IN ('youtube','jamendo')),
    public_recording_id TEXT NOT NULL CHECK(length(public_recording_id) BETWEEN 1 AND 20),
    status TEXT NOT NULL CHECK(status IN ('queued','running','accepted','cancelled','failed')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 3),
    next_attempt_at INTEGER NOT NULL DEFAULT (unixepoch()),
    lease_token TEXT,
    lease_until INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(user_id,submission_id),
    CHECK((lease_token IS NULL) = (lease_until IS NULL)),
    CHECK((status = 'running') = (lease_token IS NOT NULL))
);
CREATE INDEX community_report_queue ON community_reports(service_key,status,next_attempt_at,created_at);
CREATE INDEX community_report_account ON community_reports(user_id,status,updated_at);
CREATE INDEX community_report_session ON community_reports(session_hash) WHERE status IN ('queued','running');

CREATE TRIGGER community_report_identity BEFORE UPDATE ON community_reports
WHEN old.user_id IS NOT new.user_id OR old.submission_id IS NOT new.submission_id
    OR old.service_key IS NOT new.service_key OR old.session_hash IS NOT new.session_hash
    OR old.source_id IS NOT new.source_id OR old.provider IS NOT new.provider
    OR old.public_recording_id IS NOT new.public_recording_id OR old.created_at IS NOT new.created_at BEGIN
    SELECT RAISE(ABORT, 'Community report identity is immutable');
END;
CREATE TRIGGER community_reporting_disabled AFTER UPDATE OF playback_reporting ON account_community_preferences
WHEN new.playback_reporting = 0 BEGIN
    UPDATE community_reports SET status = 'cancelled',lease_token = NULL,lease_until = NULL,updated_at = unixepoch()
    WHERE user_id = new.user_id AND status IN ('queued','running');
END;
CREATE TRIGGER community_reporting_logout AFTER DELETE ON sessions BEGIN
    UPDATE community_reports SET status = 'cancelled',lease_token = NULL,lease_until = NULL,updated_at = unixepoch()
    WHERE session_hash = old.token_hash AND status IN ('queued','running');
END;
CREATE TRIGGER community_reporting_session_changed AFTER UPDATE ON sessions
WHEN new.expires_at <= unixepoch() OR new.token_hash IS NOT old.token_hash OR new.user_id IS NOT old.user_id BEGIN
    UPDATE community_reports SET status = 'cancelled',lease_token = NULL,lease_until = NULL,updated_at = unixepoch()
    WHERE session_hash = old.token_hash AND status IN ('queued','running');
END;
