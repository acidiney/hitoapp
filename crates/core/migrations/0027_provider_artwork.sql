CREATE TABLE artwork_assets (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content_hash TEXT NOT NULL CHECK(length(content_hash) = 64),
    content_type TEXT NOT NULL CHECK(content_type IN ('image/png','image/jpeg')),
    width INTEGER NOT NULL CHECK(width BETWEEN 1 AND 1024),
    height INTEGER NOT NULL CHECK(height BETWEEN 1 AND 1024),
    bytes BLOB NOT NULL CHECK(length(bytes) BETWEEN 1 AND 2097152),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(user_id,content_hash)
);

CREATE TABLE recording_artwork (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    observation_id TEXT NOT NULL REFERENCES metadata_observations(id),
    fetch_id TEXT NOT NULL UNIQUE REFERENCES artwork_fetches(id),
    content_hash TEXT NOT NULL,
    PRIMARY KEY(user_id,recording_id),
    FOREIGN KEY(user_id,content_hash) REFERENCES artwork_assets(user_id,content_hash)
);
CREATE INDEX recording_artwork_asset ON recording_artwork(user_id,content_hash);
CREATE TRIGGER recording_artwork_owner BEFORE INSERT ON recording_artwork
WHEN NOT EXISTS(SELECT 1 FROM recordings r JOIN libraries l ON l.id = r.library_id
    JOIN library_members m ON m.library_id = l.id AND m.user_id = new.user_id
    JOIN metadata_observations o ON o.id = new.observation_id AND o.recording_id = r.id
    JOIN artwork_fetches fetch ON fetch.id = new.fetch_id AND fetch.user_id = new.user_id
        AND fetch.recording_id = r.id AND fetch.observation_id = o.id
        AND fetch.status IN ('running','ready')
    WHERE r.id = new.recording_id AND l.owner_id = new.user_id) BEGIN
    SELECT RAISE(ABORT, 'Artwork requires its account recording');
END;
CREATE TRIGGER recording_artwork_update BEFORE UPDATE ON recording_artwork
WHEN old.user_id IS NOT new.user_id OR old.recording_id IS NOT new.recording_id
    OR NOT EXISTS(SELECT 1 FROM metadata_observations o WHERE o.id = new.observation_id
        AND o.recording_id = new.recording_id)
    OR NOT EXISTS(SELECT 1 FROM artwork_fetches fetch WHERE fetch.id = new.fetch_id
        AND fetch.user_id = new.user_id AND fetch.recording_id = new.recording_id
        AND fetch.observation_id = new.observation_id AND fetch.status IN ('running','ready')) BEGIN
    SELECT RAISE(ABORT, 'Artwork ownership is immutable');
END;

CREATE TABLE artwork_fetches (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    session_hash TEXT NOT NULL,
    observation_id TEXT NOT NULL REFERENCES metadata_observations(id),
    binding_id TEXT NOT NULL REFERENCES provider_items(id),
    binding_revision INTEGER NOT NULL CHECK(binding_revision > 0),
    installation_id TEXT NOT NULL REFERENCES plugin_installations(id),
    installation_revision INTEGER NOT NULL CHECK(installation_revision > 0),
    service_id TEXT NOT NULL REFERENCES plugin_services(id),
    service_revision INTEGER NOT NULL CHECK(service_revision > 0),
    status TEXT NOT NULL CHECK(status IN ('queued','running','ready','failed','cancelled')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 3),
    reservation_bytes INTEGER NOT NULL,
    next_attempt_at INTEGER NOT NULL DEFAULT (unixepoch()),
    lease_token TEXT,
    lease_until INTEGER,
    error TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK((status IN ('queued','running') AND reservation_bytes = 2097152)
        OR (status IN ('ready','failed','cancelled') AND reservation_bytes = 0)),
    CHECK((lease_token IS NULL) = (lease_until IS NULL)),
    CHECK(status != 'running' OR lease_token IS NOT NULL),
    CHECK(status != 'queued' OR lease_token IS NULL)
);
CREATE UNIQUE INDEX artwork_pending_recording ON artwork_fetches(user_id,recording_id)
    WHERE status IN ('queued','running');
CREATE INDEX artwork_queue ON artwork_fetches(status,next_attempt_at,created_at,id);
CREATE INDEX artwork_expired_lease ON artwork_fetches(lease_until) WHERE lease_token IS NOT NULL;
CREATE INDEX artwork_account ON artwork_fetches(user_id,status,updated_at,id);
CREATE INDEX artwork_session ON artwork_fetches(session_hash) WHERE status IN ('queued','running');

CREATE VIEW current_artwork_fetch_authority AS
SELECT j.id FROM artwork_fetches j
JOIN sessions session ON session.token_hash = j.session_hash AND session.user_id = j.user_id
    AND session.expires_at > unixepoch()
JOIN recordings r ON r.id = j.recording_id AND r.metadata_observation_id = j.observation_id
    AND r.metadata_binding_id = j.binding_id
JOIN libraries l ON l.id = r.library_id AND l.owner_id = j.user_id
JOIN library_members m ON m.library_id = l.id AND m.user_id = j.user_id
JOIN metadata_observations o ON o.id = j.observation_id AND o.recording_id = r.id
    AND o.origin = 'provider_response' AND o.provider_binding_id = j.binding_id
    AND o.plugin_installation_id = j.installation_id
JOIN provider_items binding ON binding.id = j.binding_id AND binding.recording_id = r.id
    AND binding.installation_id = j.installation_id AND binding.revision = j.binding_revision
    AND binding.retired = 0 AND binding.provider_recording_id = o.provider_recording_id
JOIN plugin_installations p ON p.id = j.installation_id AND p.user_id = j.user_id
    AND p.active = 1 AND p.revision = j.installation_revision AND p.service_id = j.service_id
    AND json_extract(p.manifest,'$.id') = o.provider
JOIN plugin_services service ON service.id = j.service_id AND service.active = 1
    AND service.revision = j.service_revision
WHERE EXISTS(SELECT 1 FROM json_each(p.manifest,'$.capabilities') WHERE value = 'artwork');

CREATE TRIGGER artwork_fetch_identity BEFORE UPDATE ON artwork_fetches
WHEN old.id IS NOT new.id OR old.user_id IS NOT new.user_id
    OR old.recording_id IS NOT new.recording_id OR old.session_hash IS NOT new.session_hash
    OR old.observation_id IS NOT new.observation_id OR old.binding_id IS NOT new.binding_id
    OR old.binding_revision IS NOT new.binding_revision OR old.installation_id IS NOT new.installation_id
    OR old.installation_revision IS NOT new.installation_revision OR old.service_id IS NOT new.service_id
    OR old.service_revision IS NOT new.service_revision OR old.created_at IS NOT new.created_at BEGIN
    SELECT RAISE(ABORT, 'Artwork fetch authority is immutable');
END;

CREATE TRIGGER artwork_logout AFTER DELETE ON sessions BEGIN
    UPDATE artwork_fetches SET status = 'cancelled',reservation_bytes = 0,error = 'session_ended',
        updated_at = unixepoch()
    WHERE session_hash = old.token_hash AND status IN ('queued','running');
END;
CREATE TRIGGER artwork_session_expiry AFTER UPDATE ON sessions
WHEN new.expires_at <= unixepoch() OR new.token_hash IS NOT old.token_hash
    OR new.user_id IS NOT old.user_id BEGIN
    UPDATE artwork_fetches SET status = 'cancelled',reservation_bytes = 0,error = 'session_ended',
        updated_at = unixepoch()
    WHERE session_hash = old.token_hash AND status IN ('queued','running');
END;
