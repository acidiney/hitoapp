CREATE TABLE community_corrections (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    submission_id TEXT NOT NULL CHECK(length(submission_id) = 36),
    service_key TEXT NOT NULL CHECK(length(service_key) = 64),
    session_hash TEXT NOT NULL,
    left_source_id TEXT NOT NULL REFERENCES sources(id),
    right_source_id TEXT NOT NULL REFERENCES sources(id),
    left_provider TEXT NOT NULL CHECK(left_provider IN ('youtube','jamendo')),
    left_recording_id TEXT NOT NULL CHECK(length(left_recording_id) BETWEEN 1 AND 20),
    right_provider TEXT NOT NULL CHECK(right_provider IN ('youtube','jamendo')),
    right_recording_id TEXT NOT NULL CHECK(length(right_recording_id) BETWEEN 1 AND 20),
    status TEXT NOT NULL CHECK(status IN ('queued','running','accepted','cancelled','failed')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 3),
    next_attempt_at INTEGER NOT NULL DEFAULT (unixepoch()),
    lease_token TEXT,
    lease_until INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(user_id,submission_id),
    CHECK(left_source_id != right_source_id),
    CHECK((lease_token IS NULL) = (lease_until IS NULL)),
    CHECK((status = 'running') = (lease_token IS NOT NULL))
);
CREATE INDEX community_correction_queue ON community_corrections(service_key,status,next_attempt_at,created_at);
CREATE INDEX community_correction_account ON community_corrections(user_id,status,updated_at);
CREATE INDEX community_correction_session ON community_corrections(session_hash) WHERE status IN ('queued','running');

CREATE TRIGGER community_correction_identity BEFORE UPDATE ON community_corrections
WHEN old.user_id IS NOT new.user_id OR old.submission_id IS NOT new.submission_id
    OR old.service_key IS NOT new.service_key OR old.session_hash IS NOT new.session_hash
    OR old.left_source_id IS NOT new.left_source_id OR old.right_source_id IS NOT new.right_source_id
    OR old.left_provider IS NOT new.left_provider OR old.left_recording_id IS NOT new.left_recording_id
    OR old.right_provider IS NOT new.right_provider OR old.right_recording_id IS NOT new.right_recording_id
    OR old.created_at IS NOT new.created_at BEGIN
    SELECT RAISE(ABORT, 'Community correction identity is immutable');
END;
CREATE TRIGGER community_corrections_disabled AFTER UPDATE OF metadata_corrections ON account_community_preferences
WHEN new.metadata_corrections = 0 BEGIN
    UPDATE community_corrections SET status = 'cancelled',lease_token = NULL,lease_until = NULL,updated_at = unixepoch()
    WHERE user_id = new.user_id AND status IN ('queued','running');
END;
CREATE TRIGGER community_corrections_logout AFTER DELETE ON sessions BEGIN
    UPDATE community_corrections SET status = 'cancelled',lease_token = NULL,lease_until = NULL,updated_at = unixepoch()
    WHERE session_hash = old.token_hash AND status IN ('queued','running');
END;
CREATE TRIGGER community_corrections_session_changed AFTER UPDATE ON sessions
WHEN new.expires_at <= unixepoch() OR new.token_hash IS NOT old.token_hash OR new.user_id IS NOT old.user_id BEGIN
    UPDATE community_corrections SET status = 'cancelled',lease_token = NULL,lease_until = NULL,updated_at = unixepoch()
    WHERE session_hash = old.token_hash AND status IN ('queued','running');
END;
