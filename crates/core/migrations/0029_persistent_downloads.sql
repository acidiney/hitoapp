CREATE TABLE download_files (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES users(id),
    host_device TEXT NOT NULL,
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    source_id TEXT UNIQUE REFERENCES sources(id),
    state TEXT NOT NULL CHECK(state IN ('staging','ready','deleting','deleted')),
    generation INTEGER NOT NULL DEFAULT 1 CHECK(generation = 1),
    attempt_id TEXT NOT NULL,
    checksum TEXT CHECK(checksum IS NULL OR (length(checksum) = 64 AND checksum NOT GLOB '*[^0-9a-f]*')),
    byte_length INTEGER CHECK(byte_length > 0 AND byte_length <= 2147483648),
    mime TEXT CHECK(mime IS NULL OR mime IN ('audio/wav','audio/mpeg','audio/flac','audio/ogg','audio/opus','audio/mp4','audio/aac','audio/aiff','audio/x-ms-wma')),
    cleanup_after INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK(state != 'ready' OR (source_id IS NOT NULL AND checksum IS NOT NULL AND byte_length IS NOT NULL AND mime IS NOT NULL)),
    UNIQUE(id,account_id,recording_id,host_device)
);
CREATE INDEX download_file_cleanup ON download_files(host_device,state,cleanup_after,created_at,id);

CREATE TABLE download_intents (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    library_id TEXT NOT NULL REFERENCES libraries(id),
    access_grant_id TEXT NOT NULL,
    host_device TEXT NOT NULL,
    generation INTEGER NOT NULL DEFAULT 1 CHECK(generation > 0),
    state TEXT NOT NULL CHECK(state IN ('queued','running','verifying','ready','failed','removed')),
    file_id TEXT REFERENCES download_files(id) ON DELETE SET NULL,
    provider_source_id TEXT NOT NULL REFERENCES sources(id),
    binding_id TEXT NOT NULL REFERENCES provider_items(id),
    binding_revision INTEGER NOT NULL CHECK(binding_revision > 0),
    remote_source_id TEXT NOT NULL,
    installation_id TEXT NOT NULL REFERENCES plugin_installations(id),
    installation_revision INTEGER NOT NULL CHECK(installation_revision > 0),
    service_id TEXT NOT NULL REFERENCES plugin_services(id),
    service_revision INTEGER NOT NULL CHECK(service_revision > 0),
    credential_digest TEXT NOT NULL CHECK(length(credential_digest) = 64),
    audio_generation INTEGER NOT NULL CHECK(audio_generation >= 0),
    bytes_received INTEGER NOT NULL DEFAULT 0 CHECK(bytes_received BETWEEN 0 AND 2147483648),
    total_bytes INTEGER CHECK(total_bytes BETWEEN 1 AND 2147483648),
    error TEXT CHECK(error IS NULL OR error IN ('provider_unavailable','transfer_failed','authority_changed','lease_expired','file_unavailable')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK(state != 'ready' OR file_id IS NOT NULL),
    UNIQUE(id,account_id)
);
CREATE UNIQUE INDEX download_current_intent ON download_intents(account_id,recording_id,host_device) WHERE state != 'removed';
CREATE INDEX download_file_references ON download_intents(file_id,state);
CREATE INDEX download_intent_account ON download_intents(account_id,host_device,state,id);
CREATE INDEX download_intent_queued_host ON download_intents(host_device,id) WHERE state='queued';
CREATE TRIGGER download_file_owner BEFORE UPDATE OF file_id ON download_intents
WHEN new.file_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM download_files f WHERE f.id = new.file_id
    AND f.account_id = new.account_id AND f.recording_id = new.recording_id AND f.host_device = new.host_device AND f.state IN ('staging','ready')) BEGIN
    SELECT RAISE(ABORT, 'Download file ownership does not match');
END;
CREATE TRIGGER download_file_owner_insert BEFORE INSERT ON download_intents
WHEN new.file_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM download_files f WHERE f.id = new.file_id
    AND f.account_id = new.account_id AND f.recording_id = new.recording_id AND f.host_device = new.host_device AND f.state = 'ready') BEGIN
    SELECT RAISE(ABORT, 'Download file ownership does not match');
END;
CREATE TRIGGER download_intent_identity BEFORE UPDATE ON download_intents
WHEN old.id IS NOT new.id OR old.account_id IS NOT new.account_id OR old.recording_id IS NOT new.recording_id
    OR old.library_id IS NOT new.library_id OR old.access_grant_id IS NOT new.access_grant_id OR old.host_device IS NOT new.host_device
    OR old.provider_source_id IS NOT new.provider_source_id OR old.binding_id IS NOT new.binding_id OR old.binding_revision IS NOT new.binding_revision
    OR old.remote_source_id IS NOT new.remote_source_id OR old.installation_id IS NOT new.installation_id
    OR old.installation_revision IS NOT new.installation_revision OR old.service_id IS NOT new.service_id
    OR old.service_revision IS NOT new.service_revision OR old.credential_digest IS NOT new.credential_digest OR old.audio_generation IS NOT new.audio_generation
    OR (old.state = 'removed' AND new.state != 'removed') BEGIN
    SELECT RAISE(ABORT, 'Download intent identity is immutable');
END;

CREATE TABLE download_receipts (
    account_id TEXT NOT NULL REFERENCES users(id),
    request_id TEXT NOT NULL,
    request_digest TEXT NOT NULL CHECK(length(request_digest) = 64),
    intent_id TEXT NOT NULL REFERENCES download_intents(id),
    PRIMARY KEY(account_id,request_id)
);
CREATE TRIGGER download_receipt_retained BEFORE DELETE ON download_receipts BEGIN
    SELECT RAISE(ABORT, 'Download receipts must be retained');
END;
CREATE TRIGGER download_receipt_immutable BEFORE UPDATE ON download_receipts BEGIN
    SELECT RAISE(ABORT, 'Download receipts are immutable');
END;
CREATE TABLE download_jobs (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES users(id),
    intent_id TEXT NOT NULL REFERENCES download_intents(id),
    generation INTEGER NOT NULL CHECK(generation > 0),
    status TEXT NOT NULL CHECK(status IN ('queued','running','complete','failed','cancelled')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 3),
    next_attempt_at INTEGER NOT NULL DEFAULT (unixepoch()),
    lease_token TEXT,
    lease_until INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK((lease_token IS NULL) = (lease_until IS NULL)),
    CHECK((status = 'running') = (lease_token IS NOT NULL)),
    FOREIGN KEY(intent_id,account_id) REFERENCES download_intents(id,account_id)
);
CREATE UNIQUE INDEX download_pending_job ON download_jobs(intent_id) WHERE status IN ('queued','running');
CREATE INDEX download_job_queue ON download_jobs(status,next_attempt_at,created_at,id);
CREATE INDEX download_job_account ON download_jobs(account_id,status);
CREATE INDEX download_job_expired ON download_jobs(lease_until) WHERE lease_token IS NOT NULL;

CREATE VIEW current_download_authority AS
SELECT i.id FROM download_intents i
JOIN users account ON account.id = i.account_id
JOIN recordings recording ON recording.id = i.recording_id AND recording.library_id = i.library_id
JOIN library_access_grants access ON access.library_id = i.library_id AND access.user_id = i.account_id AND access.id = i.access_grant_id
JOIN sources source ON source.id = i.provider_source_id AND source.recording_id = i.recording_id AND source.ready = 1
JOIN provider_sources ps ON ps.source_id = source.id AND ps.recording_id = i.recording_id
    AND ps.binding_id = i.binding_id AND ps.installation_id = i.installation_id AND ps.provider_source_id = i.remote_source_id
JOIN provider_items binding ON binding.id = i.binding_id AND binding.recording_id = i.recording_id
    AND binding.installation_id = i.installation_id AND binding.revision = i.binding_revision AND binding.retired = 0
JOIN provider_source_state source_state ON source_state.binding_id = binding.id AND source_state.generation = ps.generation
JOIN plugin_installations installation ON installation.id = i.installation_id AND installation.user_id = i.account_id
    AND installation.active = 1 AND installation.revision = i.installation_revision AND installation.service_id = i.service_id
    AND installation.audio_generation = i.audio_generation
JOIN plugin_services service ON service.id = i.service_id AND service.active = 1 AND service.revision = i.service_revision
WHERE i.state IN ('queued','running','verifying')
    AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value = 'sources');
