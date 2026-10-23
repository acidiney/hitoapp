PRAGMA defer_foreign_keys = ON;
CREATE TEMP TABLE hito_unavailable_resolutions AS SELECT rowid AS preserved_rowid,* FROM source_resolutions;
DROP TABLE source_resolutions;
CREATE TABLE source_resolutions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    target_digest TEXT NOT NULL CHECK(length(target_digest) = 64),
    choice_revision INTEGER NOT NULL CHECK(choice_revision >= 0),
    outcome TEXT CHECK(outcome IN ('ready','no_suitable_source','provider_unavailable','protected_choice','cancelled')),
    lease_token TEXT,
    lease_until INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    completed_at INTEGER,
    CHECK((lease_token IS NULL) = (lease_until IS NULL)),
    CHECK((outcome IS NULL) = (lease_token IS NOT NULL)),
    CHECK((outcome IS NULL) = (completed_at IS NULL))
);
INSERT INTO source_resolutions(rowid,id,user_id,recording_id,target_digest,choice_revision,outcome,lease_token,lease_until,created_at,completed_at)
SELECT preserved_rowid,id,user_id,recording_id,target_digest,choice_revision,outcome,lease_token,lease_until,created_at,completed_at
FROM hito_unavailable_resolutions;
DROP TABLE hito_unavailable_resolutions;
CREATE UNIQUE INDEX source_resolution_running_account ON source_resolutions(user_id) WHERE outcome IS NULL;
CREATE INDEX source_resolution_running ON source_resolutions(lease_until,id) WHERE outcome IS NULL;
CREATE INDEX source_resolution_recording ON source_resolutions(user_id,recording_id,created_at);
CREATE TRIGGER source_resolution_identity_immutable BEFORE UPDATE OF id,user_id,recording_id,target_digest,choice_revision,created_at ON source_resolutions BEGIN
    SELECT RAISE(ABORT, 'Source resolution identity is immutable');
END;
CREATE TRIGGER source_resolution_terminal_immutable BEFORE UPDATE ON source_resolutions WHEN old.outcome IS NOT NULL BEGIN
    SELECT RAISE(ABORT, 'Source resolution terminal results are immutable');
END;
CREATE TEMP TABLE hito_unavailable_fk_guard (value INTEGER NOT NULL CHECK(value = 0));
INSERT INTO hito_unavailable_fk_guard SELECT 1 FROM pragma_foreign_key_check;
DROP TABLE hito_unavailable_fk_guard;
PRAGMA defer_foreign_keys = OFF;
