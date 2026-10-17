CREATE TABLE source_preparations (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    source_id TEXT NOT NULL REFERENCES sources(id),
    library_id TEXT NOT NULL REFERENCES libraries(id),
    access_grant TEXT NOT NULL,
    binding_id TEXT NOT NULL REFERENCES provider_items(id),
    binding_revision INTEGER NOT NULL CHECK(binding_revision > 0),
    provider_recording_id TEXT NOT NULL,
    remote_source_id TEXT NOT NULL,
    installation_id TEXT NOT NULL REFERENCES plugin_installations(id),
    installation_revision INTEGER NOT NULL CHECK(installation_revision > 0),
    service_id TEXT NOT NULL REFERENCES plugin_services(id),
    service_revision INTEGER NOT NULL CHECK(service_revision > 0),
    provider TEXT NOT NULL,
    credential BLOB,
    audio_generation INTEGER NOT NULL CHECK(audio_generation >= 0),
    phase TEXT NOT NULL DEFAULT 'queued'
        CHECK(phase IN ('queued','submitting','preparing','ready','blocked','failed','submission_unknown','cancelled')),
    submission_state TEXT NOT NULL DEFAULT 'unsubmitted'
        CHECK(submission_state IN ('unsubmitted','submitting','submitted','unknown')),
    remote_outcome TEXT CHECK(remote_outcome IN ('ready','failed')),
    authority_cancelled INTEGER NOT NULL DEFAULT 0 CHECK(authority_cancelled IN (0,1)),
    remote_handle TEXT CHECK(remote_handle IS NULL OR length(remote_handle) BETWEEN 1 AND 2048),
    progress_basis_points INTEGER CHECK(progress_basis_points BETWEEN 0 AND 10000),
    eta_seconds INTEGER CHECK(eta_seconds BETWEEN 0 AND 604800),
    reason TEXT CHECK(reason IS NULL OR reason IN (
        'authorization_required','quota_reached','rate_limited','provider_restricted',
        'source_unavailable','source_changed','unsupported','provider_unavailable','invalid_response')),
    next_check_at INTEGER DEFAULT (unixepoch()),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts >= 0),
    begin_attempts INTEGER NOT NULL DEFAULT 0 CHECK(begin_attempts BETWEEN 0 AND 3),
    poll_failures INTEGER NOT NULL DEFAULT 0 CHECK(poll_failures >= 0),
    dispatch_id TEXT,
    lease_token TEXT,
    lease_until INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK((lease_token IS NULL) = (lease_until IS NULL)),
    CHECK(remote_handle IS NULL OR submission_state = 'submitted'),
    CHECK(remote_outcome IS NULL OR submission_state = 'submitted'),
    CHECK((phase = 'submitting') = (submission_state = 'submitting')),
    CHECK(authority_cancelled = 0 OR next_check_at IS NULL)
);

CREATE UNIQUE INDEX source_preparation_retained_submission ON source_preparations(user_id,source_id)
    WHERE remote_outcome IS NULL AND (
        phase IN ('queued','submitting','preparing','blocked','submission_unknown')
        OR submission_state != 'unsubmitted');
CREATE UNIQUE INDEX source_preparation_unresolved_origin ON source_preparations(
    user_id,service_id,provider,provider_recording_id,remote_source_id)
    WHERE submission_state IN ('submitting','unknown')
        OR (submission_state = 'submitted' AND remote_outcome IS NULL);
CREATE INDEX source_preparation_latest ON source_preparations(user_id,recording_id,source_id,created_at);
CREATE INDEX source_preparation_due ON source_preparations(next_check_at,created_at,id)
    WHERE authority_cancelled = 0 AND lease_token IS NULL AND phase IN ('queued','preparing','blocked');
CREATE INDEX source_preparation_expired ON source_preparations(lease_until,id) WHERE lease_token IS NOT NULL;
CREATE INDEX source_preparation_leased_account ON source_preparations(user_id) WHERE lease_token IS NOT NULL;
CREATE INDEX source_preparation_pending ON source_preparations(user_id)
    WHERE phase IN ('queued','submitting','preparing','blocked','submission_unknown');
CREATE INDEX source_preparation_authority_pending ON source_preparations(created_at,id)
    WHERE authority_cancelled = 0 AND phase IN ('queued','submitting','preparing','blocked','submission_unknown');

CREATE TABLE source_preparation_receipts (
    user_id TEXT NOT NULL REFERENCES users(id),
    request_id TEXT NOT NULL,
    request_digest TEXT NOT NULL CHECK(length(request_digest) = 64 AND request_digest NOT GLOB '*[^0-9a-f]*'),
    preparation_id TEXT NOT NULL REFERENCES source_preparations(id),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(user_id,request_id)
);
CREATE INDEX source_preparation_receipt_intent ON source_preparation_receipts(preparation_id);

CREATE TRIGGER immutable_source_preparation_receipt BEFORE UPDATE ON source_preparation_receipts BEGIN
    SELECT RAISE(ABORT, 'Source preparation receipts are immutable');
END;
CREATE TRIGGER immutable_source_preparation_remote_outcome BEFORE UPDATE OF remote_outcome ON source_preparations
    WHEN OLD.remote_outcome IS NOT NULL AND NEW.remote_outcome IS NOT OLD.remote_outcome BEGIN
    SELECT RAISE(ABORT, 'Source preparation terminal outcomes are immutable');
END;
CREATE TRIGGER immutable_source_preparation_target BEFORE UPDATE OF
    id,user_id,recording_id,source_id,library_id,access_grant,binding_id,binding_revision,
    provider_recording_id,remote_source_id,installation_id,installation_revision,
    service_id,service_revision,provider,credential,audio_generation
    ON source_preparations BEGIN
    SELECT RAISE(ABORT, 'Source preparation targets are immutable');
END;

CREATE VIEW current_source_preparation_authority AS
SELECT preparation.id FROM source_preparations preparation
JOIN users account ON account.id = preparation.user_id
JOIN recordings recording ON recording.id = preparation.recording_id AND recording.library_id = preparation.library_id
JOIN library_access_grants access ON access.library_id = preparation.library_id
    AND access.user_id = preparation.user_id AND access.id = preparation.access_grant
JOIN sources source ON source.id = preparation.source_id AND source.recording_id = preparation.recording_id
JOIN provider_sources remote ON remote.source_id = source.id AND remote.recording_id = source.recording_id
    AND remote.binding_id = preparation.binding_id AND remote.installation_id = preparation.installation_id
    AND remote.provider_source_id = preparation.remote_source_id
JOIN provider_items binding ON binding.id = preparation.binding_id AND binding.retired = 0
    AND binding.recording_id = preparation.recording_id AND binding.revision = preparation.binding_revision
    AND binding.installation_id = preparation.installation_id AND binding.provider_recording_id = preparation.provider_recording_id
JOIN provider_source_state snapshot ON snapshot.binding_id = binding.id AND snapshot.generation = remote.generation
JOIN plugin_installations installation ON installation.id = preparation.installation_id
    AND installation.user_id = preparation.user_id AND installation.active = 1
    AND installation.revision = preparation.installation_revision AND installation.service_id = preparation.service_id
    AND installation.credential IS preparation.credential AND installation.audio_generation = preparation.audio_generation
    AND json_extract(installation.manifest,'$.id') = preparation.provider
JOIN plugin_services service ON service.id = preparation.service_id AND service.active = 1
    AND service.revision = preparation.service_revision
WHERE preparation.authority_cancelled = 0
    AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value = 'sources')
    AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value = 'source_preparation');
