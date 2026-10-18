PRAGMA defer_foreign_keys = ON;
DROP VIEW current_artwork_fetch_authority;
DROP TRIGGER recording_artwork_owner;
DROP TRIGGER recording_artwork_update;
DROP TRIGGER artwork_logout;
DROP TRIGGER artwork_session_expiry;

CREATE TABLE artwork_fetches_next (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    recording_id TEXT REFERENCES recordings(id),
    session_hash TEXT NOT NULL,
    observation_id TEXT REFERENCES metadata_observations(id),
    binding_id TEXT REFERENCES provider_items(id),
    binding_revision INTEGER CHECK(binding_revision > 0),
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
    entity_kind TEXT NOT NULL DEFAULT 'recording' CHECK(entity_kind IN ('recording','release','playlist')),
    release_id TEXT REFERENCES releases(id),
    provider_release_id TEXT,
    playlist_id TEXT REFERENCES playlists(id),
    provider_playlist_id TEXT,
    playlist_provider TEXT,
    playlist_snapshot TEXT,
    playlist_imported_at INTEGER,
    retained INTEGER NOT NULL DEFAULT 0 CHECK(retained IN (0,1)),
    CHECK((status IN ('queued','running') AND reservation_bytes = 2097152)
        OR (status IN ('ready','failed','cancelled') AND reservation_bytes = 0)),
    CHECK((lease_token IS NULL) = (lease_until IS NULL)),
    CHECK(status != 'running' OR lease_token IS NOT NULL),
    CHECK(status != 'queued' OR lease_token IS NULL),
    CHECK((entity_kind IN ('recording','release') AND recording_id IS NOT NULL
            AND observation_id IS NOT NULL AND binding_id IS NOT NULL AND binding_revision IS NOT NULL
            AND playlist_id IS NULL AND provider_playlist_id IS NULL AND playlist_provider IS NULL
            AND playlist_snapshot IS NULL AND playlist_imported_at IS NULL)
        OR (entity_kind = 'playlist' AND recording_id IS NULL AND observation_id IS NULL
            AND binding_id IS NULL AND binding_revision IS NULL AND playlist_id IS NOT NULL
            AND provider_playlist_id IS NOT NULL AND length(provider_playlist_id) BETWEEN 1 AND 256
            AND playlist_provider IS NOT NULL AND playlist_snapshot IS NOT NULL AND playlist_imported_at IS NOT NULL)),
    CHECK((entity_kind = 'release' AND release_id IS NOT NULL AND provider_release_id IS NOT NULL
            AND length(provider_release_id) BETWEEN 1 AND 256)
        OR (entity_kind != 'release' AND release_id IS NULL AND provider_release_id IS NULL))
);
INSERT INTO artwork_fetches_next(rowid,id,user_id,recording_id,session_hash,observation_id,binding_id,binding_revision,installation_id,installation_revision,service_id,service_revision,status,attempts,reservation_bytes,next_attempt_at,lease_token,lease_until,error,created_at,updated_at,retained)
SELECT rowid,id,user_id,recording_id,session_hash,observation_id,binding_id,binding_revision,installation_id,installation_revision,service_id,service_revision,status,attempts,reservation_bytes,next_attempt_at,lease_token,lease_until,error,created_at,updated_at,
    EXISTS(SELECT 1 FROM recording_artwork current WHERE current.fetch_id = artwork_fetches.id) FROM artwork_fetches;
CREATE TABLE recording_artwork_next (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    observation_id TEXT NOT NULL REFERENCES metadata_observations(id),
    fetch_id TEXT NOT NULL UNIQUE REFERENCES artwork_fetches_next(id),
    content_hash TEXT NOT NULL,
    PRIMARY KEY(user_id,recording_id),
    FOREIGN KEY(user_id,content_hash) REFERENCES artwork_assets(user_id,content_hash)
);
INSERT INTO recording_artwork_next(rowid,user_id,recording_id,observation_id,fetch_id,content_hash)
SELECT rowid,user_id,recording_id,observation_id,fetch_id,content_hash FROM recording_artwork;
DROP TABLE recording_artwork;
DROP TABLE artwork_fetches;
ALTER TABLE artwork_fetches_next RENAME TO artwork_fetches;
ALTER TABLE recording_artwork_next RENAME TO recording_artwork;
CREATE INDEX recording_artwork_asset ON recording_artwork(user_id,content_hash);
CREATE UNIQUE INDEX artwork_pending_recording ON artwork_fetches(user_id,recording_id)
    WHERE entity_kind = 'recording' AND status IN ('queued','running');
CREATE UNIQUE INDEX artwork_pending_release ON artwork_fetches(user_id,release_id)
    WHERE entity_kind = 'release' AND status IN ('queued','running');
CREATE UNIQUE INDEX artwork_pending_playlist ON artwork_fetches(user_id,playlist_id)
    WHERE entity_kind = 'playlist' AND status IN ('queued','running');
CREATE INDEX artwork_queue ON artwork_fetches(status,next_attempt_at,created_at,id);
CREATE INDEX artwork_expired_lease ON artwork_fetches(lease_until) WHERE lease_token IS NOT NULL;
CREATE INDEX artwork_account ON artwork_fetches(user_id,status,updated_at,id);
CREATE INDEX artwork_session ON artwork_fetches(session_hash) WHERE status IN ('queued','running');
CREATE INDEX artwork_binding ON artwork_fetches(binding_id);
CREATE INDEX artwork_history ON artwork_fetches(updated_at DESC,id DESC)
    WHERE retained = 0 AND status IN ('ready','failed','cancelled') AND lease_token IS NULL;
CREATE INDEX artwork_history_account ON artwork_fetches(user_id,updated_at DESC,id DESC)
    WHERE retained = 0 AND status IN ('ready','failed','cancelled') AND lease_token IS NULL;

CREATE TRIGGER artwork_fetch_identity BEFORE UPDATE ON artwork_fetches
WHEN old.id IS NOT new.id OR old.user_id IS NOT new.user_id
    OR old.recording_id IS NOT new.recording_id OR old.session_hash IS NOT new.session_hash
    OR old.observation_id IS NOT new.observation_id OR old.binding_id IS NOT new.binding_id
    OR old.binding_revision IS NOT new.binding_revision OR old.installation_id IS NOT new.installation_id
    OR old.installation_revision IS NOT new.installation_revision OR old.service_id IS NOT new.service_id
    OR old.service_revision IS NOT new.service_revision OR old.created_at IS NOT new.created_at
    OR old.entity_kind IS NOT new.entity_kind OR old.release_id IS NOT new.release_id
    OR old.provider_release_id IS NOT new.provider_release_id OR old.playlist_id IS NOT new.playlist_id
    OR old.provider_playlist_id IS NOT new.provider_playlist_id OR old.playlist_provider IS NOT new.playlist_provider
    OR old.playlist_snapshot IS NOT new.playlist_snapshot OR old.playlist_imported_at IS NOT new.playlist_imported_at BEGIN
    SELECT RAISE(ABORT, 'Artwork fetch authority is immutable');
END;

CREATE TRIGGER artwork_fetch_target_insert BEFORE INSERT ON artwork_fetches
WHEN (new.entity_kind = 'release' AND NOT EXISTS(
    SELECT 1 FROM metadata_observations o JOIN recordings r ON r.id = o.recording_id
    JOIN releases a ON a.id = o.release_id AND a.library_id = r.library_id
    WHERE o.id = new.observation_id AND o.recording_id = new.recording_id
        AND r.metadata_observation_id = o.id AND r.release_id = new.release_id
        AND o.release_id = new.release_id
        AND json_type(o.provider_metadata,'$.release.id') = 'text'
        AND json_extract(o.provider_metadata,'$.release.id') = new.provider_release_id))
    OR (new.entity_kind = 'playlist' AND NOT EXISTS(
        SELECT 1 FROM playlists p JOIN playlist_imports origin ON origin.playlist_id = p.id
        WHERE p.id = new.playlist_id AND p.user_id = new.user_id AND p.kind = 'local' AND p.deleted = 0
            AND origin.installation_id = new.installation_id AND origin.service_id = new.service_id
            AND origin.provider = new.playlist_provider AND origin.provider_playlist_id = new.provider_playlist_id
            AND origin.snapshot = new.playlist_snapshot AND origin.imported_at = new.playlist_imported_at)) BEGIN
    SELECT RAISE(ABORT, 'Artwork requires its exact entity witness');
END;

CREATE VIEW current_artwork_fetch_connections AS
SELECT j.id,json_extract(p.manifest,'$.id') AS provider FROM artwork_fetches j
JOIN sessions session ON session.token_hash = j.session_hash AND session.user_id = j.user_id
    AND session.expires_at > unixepoch()
JOIN plugin_installations p ON p.id = j.installation_id AND p.user_id = j.user_id
    AND p.active = 1 AND p.revision = j.installation_revision AND p.service_id = j.service_id
JOIN plugin_services service ON service.id = j.service_id AND service.active = 1
    AND service.revision = j.service_revision
WHERE EXISTS(SELECT 1 FROM json_each(p.manifest,'$.capabilities') WHERE value = 'artwork');

CREATE VIEW current_artwork_fetch_authority AS
SELECT j.id FROM artwork_fetches j
JOIN current_artwork_fetch_connections connection ON connection.id = j.id
JOIN recordings r ON r.id = j.recording_id AND r.metadata_observation_id = j.observation_id
    AND r.metadata_binding_id = j.binding_id
JOIN libraries l ON l.id = r.library_id AND l.owner_id = j.user_id
JOIN library_members m ON m.library_id = l.id AND m.user_id = j.user_id
JOIN metadata_observations o ON o.id = j.observation_id AND o.recording_id = r.id
    AND o.origin = 'provider_response' AND o.provider_binding_id = j.binding_id
    AND o.plugin_installation_id = j.installation_id AND o.provider = connection.provider
JOIN provider_items binding ON binding.id = j.binding_id AND binding.recording_id = r.id
    AND binding.installation_id = j.installation_id AND binding.revision = j.binding_revision
    AND binding.retired = 0 AND binding.provider_recording_id = o.provider_recording_id
WHERE j.entity_kind = 'recording'
    OR (j.entity_kind = 'release' AND j.release_id = r.release_id AND j.release_id = o.release_id
        AND json_type(o.provider_metadata,'$.release.id') = 'text'
        AND json_extract(o.provider_metadata,'$.release.id') = j.provider_release_id)
UNION ALL
SELECT j.id FROM artwork_fetches j
JOIN current_artwork_fetch_connections connection ON connection.id = j.id AND connection.provider = j.playlist_provider
JOIN playlists p ON p.id = j.playlist_id AND p.user_id = j.user_id AND p.kind = 'local' AND p.deleted = 0
JOIN playlist_imports origin ON origin.playlist_id = p.id AND origin.installation_id = j.installation_id
    AND origin.service_id = j.service_id AND origin.provider = j.playlist_provider
    AND origin.provider_playlist_id = j.provider_playlist_id AND origin.snapshot = j.playlist_snapshot
    AND origin.imported_at = j.playlist_imported_at
WHERE j.entity_kind = 'playlist';

CREATE TRIGGER recording_artwork_owner BEFORE INSERT ON recording_artwork
WHEN NOT EXISTS(SELECT 1 FROM recordings r JOIN libraries l ON l.id = r.library_id
    JOIN library_members m ON m.library_id = l.id AND m.user_id = new.user_id
    JOIN metadata_observations o ON o.id = new.observation_id AND o.recording_id = r.id
    JOIN artwork_fetches fetch ON fetch.id = new.fetch_id AND fetch.user_id = new.user_id
        AND fetch.recording_id = r.id AND fetch.observation_id = o.id
        AND fetch.entity_kind = 'recording' AND fetch.status IN ('running','ready')
    WHERE r.id = new.recording_id AND l.owner_id = new.user_id) BEGIN
    SELECT RAISE(ABORT, 'Artwork requires its account recording');
END;
CREATE TRIGGER recording_artwork_update BEFORE UPDATE ON recording_artwork
WHEN old.user_id IS NOT new.user_id OR old.recording_id IS NOT new.recording_id
    OR NOT EXISTS(SELECT 1 FROM metadata_observations o WHERE o.id = new.observation_id
        AND o.recording_id = new.recording_id)
    OR NOT EXISTS(SELECT 1 FROM artwork_fetches fetch WHERE fetch.id = new.fetch_id
        AND fetch.user_id = new.user_id AND fetch.recording_id = new.recording_id
        AND fetch.observation_id = new.observation_id AND fetch.entity_kind = 'recording' AND fetch.status IN ('running','ready')) BEGIN
    SELECT RAISE(ABORT, 'Artwork ownership is immutable');
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

CREATE TABLE release_artwork (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    release_id TEXT NOT NULL REFERENCES releases(id),
    observation_id TEXT NOT NULL REFERENCES metadata_observations(id),
    fetch_id TEXT NOT NULL UNIQUE REFERENCES artwork_fetches(id),
    content_hash TEXT NOT NULL,
    PRIMARY KEY(user_id,release_id),
    FOREIGN KEY(user_id,content_hash) REFERENCES artwork_assets(user_id,content_hash)
);
CREATE INDEX release_artwork_asset ON release_artwork(user_id,content_hash);
CREATE TRIGGER release_artwork_owner BEFORE INSERT ON release_artwork
WHEN NOT EXISTS(SELECT 1 FROM releases a JOIN libraries l ON l.id = a.library_id
    JOIN library_members m ON m.library_id = l.id AND m.user_id = new.user_id
    JOIN metadata_observations o ON o.id = new.observation_id AND o.release_id = a.id
    JOIN artwork_fetches fetch ON fetch.id = new.fetch_id AND fetch.user_id = new.user_id
        AND fetch.entity_kind = 'release' AND fetch.release_id = a.id
        AND fetch.recording_id = o.recording_id AND fetch.observation_id = o.id
        AND fetch.status IN ('running','ready')
    WHERE a.id = new.release_id AND l.owner_id = new.user_id) BEGIN
    SELECT RAISE(ABORT, 'Artwork requires its account release');
END;
CREATE TRIGGER release_artwork_update BEFORE UPDATE ON release_artwork
WHEN old.user_id IS NOT new.user_id OR old.release_id IS NOT new.release_id
    OR NOT EXISTS(SELECT 1 FROM metadata_observations o WHERE o.id = new.observation_id
        AND o.release_id = new.release_id)
    OR NOT EXISTS(SELECT 1 FROM artwork_fetches fetch WHERE fetch.id = new.fetch_id
        AND fetch.user_id = new.user_id AND fetch.entity_kind = 'release'
        AND fetch.release_id = new.release_id AND fetch.observation_id = new.observation_id
        AND fetch.status IN ('running','ready')) BEGIN
    SELECT RAISE(ABORT, 'Artwork ownership is immutable');
END;
CREATE TABLE playlist_artwork (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    playlist_id TEXT NOT NULL REFERENCES playlists(id),
    fetch_id TEXT NOT NULL UNIQUE REFERENCES artwork_fetches(id),
    content_hash TEXT NOT NULL,
    PRIMARY KEY(user_id,playlist_id),
    FOREIGN KEY(user_id,content_hash) REFERENCES artwork_assets(user_id,content_hash)
);
CREATE INDEX playlist_artwork_asset ON playlist_artwork(user_id,content_hash);
CREATE TRIGGER playlist_artwork_owner BEFORE INSERT ON playlist_artwork
WHEN NOT EXISTS(SELECT 1 FROM playlists p JOIN artwork_fetches fetch ON fetch.playlist_id = p.id
    AND fetch.user_id = p.user_id AND fetch.entity_kind = 'playlist' AND fetch.status IN ('running','ready')
    WHERE p.id = new.playlist_id AND p.user_id = new.user_id AND p.deleted = 0 AND fetch.id = new.fetch_id) BEGIN
    SELECT RAISE(ABORT, 'Artwork requires its account playlist');
END;
CREATE TRIGGER playlist_artwork_update BEFORE UPDATE ON playlist_artwork
WHEN old.user_id IS NOT new.user_id OR old.playlist_id IS NOT new.playlist_id
    OR NOT EXISTS(SELECT 1 FROM artwork_fetches fetch WHERE fetch.id = new.fetch_id
        AND fetch.user_id = new.user_id AND fetch.entity_kind = 'playlist'
        AND fetch.playlist_id = new.playlist_id AND fetch.status IN ('running','ready')) BEGIN
    SELECT RAISE(ABORT, 'Artwork ownership is immutable');
END;

CREATE VIEW retained_artwork_references AS
SELECT user_id,content_hash,fetch_id FROM recording_artwork
UNION ALL
SELECT user_id,content_hash,fetch_id FROM release_artwork
UNION ALL
SELECT user_id,content_hash,fetch_id FROM playlist_artwork;

CREATE TRIGGER artwork_fetch_retention_insert BEFORE INSERT ON artwork_fetches
WHEN new.retained != 0 BEGIN
    SELECT RAISE(ABORT, 'Artwork receipt retention requires its exact reference');
END;
CREATE TRIGGER artwork_fetch_retention_update BEFORE UPDATE OF retained ON artwork_fetches
WHEN new.retained != EXISTS(SELECT 1 FROM retained_artwork_references current WHERE current.fetch_id = new.id) BEGIN
    SELECT RAISE(ABORT, 'Artwork receipt retention requires its exact reference');
END;

CREATE TRIGGER recording_artwork_retain AFTER INSERT ON recording_artwork BEGIN
    UPDATE artwork_fetches SET retained = 1 WHERE id = new.fetch_id;
END;
CREATE TRIGGER recording_artwork_replace AFTER UPDATE OF fetch_id ON recording_artwork
WHEN old.fetch_id IS NOT new.fetch_id BEGIN
    UPDATE artwork_fetches SET retained = EXISTS(SELECT 1 FROM retained_artwork_references current WHERE current.fetch_id = old.fetch_id)
        WHERE id = old.fetch_id;
    UPDATE artwork_fetches SET retained = 1 WHERE id = new.fetch_id;
END;
CREATE TRIGGER recording_artwork_unretain AFTER DELETE ON recording_artwork BEGIN
    UPDATE artwork_fetches SET retained = EXISTS(SELECT 1 FROM retained_artwork_references current WHERE current.fetch_id = old.fetch_id)
        WHERE id = old.fetch_id;
END;

CREATE TRIGGER release_artwork_retain AFTER INSERT ON release_artwork BEGIN
    UPDATE artwork_fetches SET retained = 1 WHERE id = new.fetch_id;
END;
CREATE TRIGGER release_artwork_replace AFTER UPDATE OF fetch_id ON release_artwork
WHEN old.fetch_id IS NOT new.fetch_id BEGIN
    UPDATE artwork_fetches SET retained = EXISTS(SELECT 1 FROM retained_artwork_references current WHERE current.fetch_id = old.fetch_id)
        WHERE id = old.fetch_id;
    UPDATE artwork_fetches SET retained = 1 WHERE id = new.fetch_id;
END;
CREATE TRIGGER release_artwork_unretain AFTER DELETE ON release_artwork BEGIN
    UPDATE artwork_fetches SET retained = EXISTS(SELECT 1 FROM retained_artwork_references current WHERE current.fetch_id = old.fetch_id)
        WHERE id = old.fetch_id;
END;

CREATE TRIGGER playlist_artwork_retain AFTER INSERT ON playlist_artwork BEGIN
    UPDATE artwork_fetches SET retained = 1 WHERE id = new.fetch_id;
END;
CREATE TRIGGER playlist_artwork_replace AFTER UPDATE OF fetch_id ON playlist_artwork
WHEN old.fetch_id IS NOT new.fetch_id BEGIN
    UPDATE artwork_fetches SET retained = EXISTS(SELECT 1 FROM retained_artwork_references current WHERE current.fetch_id = old.fetch_id)
        WHERE id = old.fetch_id;
    UPDATE artwork_fetches SET retained = 1 WHERE id = new.fetch_id;
END;
CREATE TRIGGER playlist_artwork_unretain AFTER DELETE ON playlist_artwork BEGIN
    UPDATE artwork_fetches SET retained = EXISTS(SELECT 1 FROM retained_artwork_references current WHERE current.fetch_id = old.fetch_id)
        WHERE id = old.fetch_id;
END;
