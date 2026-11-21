CREATE TABLE album_additions (
    id TEXT PRIMARY KEY CHECK(length(id) = 36),
    user_id TEXT NOT NULL REFERENCES users(id),
    request_id TEXT NOT NULL CHECK(length(request_id) = 36),
    request_digest TEXT NOT NULL CHECK(length(request_digest) = 64),
    release_id TEXT NOT NULL REFERENCES releases(id),
    added INTEGER NOT NULL CHECK(added >= 0),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    undone_at INTEGER,
    UNIQUE(user_id, request_id)
);
CREATE TABLE catalog_appearances (
    id TEXT PRIMARY KEY CHECK(length(id) = 36),
    addition_id TEXT NOT NULL REFERENCES album_additions(id),
    recording_id TEXT NOT NULL UNIQUE REFERENCES recordings(id),
    release_id TEXT NOT NULL REFERENCES releases(id),
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    duration_ms INTEGER NOT NULL CHECK(duration_ms >= 0),
    disc_number INTEGER,
    track_number INTEGER
);
CREATE INDEX catalog_appearance_release ON catalog_appearances(release_id);
CREATE INDEX catalog_appearance_addition ON catalog_appearances(addition_id);

DROP VIEW account_artist_releases;
DROP VIEW artist_releases;
DROP VIEW provider_release_appearances;

CREATE VIEW provider_release_appearances AS
    SELECT r.release_id, r.id AS recording_id, binding.id AS entry_id,
        observation.title, observation.artist, observation.duration_ms,
        observation.disc_number, observation.track_number
    FROM recordings r
    JOIN metadata_observations observation ON observation.id = r.metadata_observation_id
        AND observation.recording_id = r.id AND observation.release_id = r.release_id
        AND observation.provider_binding_id = r.metadata_binding_id
    JOIN provider_items binding ON binding.id = r.metadata_binding_id
        AND binding.recording_id = r.id AND binding.role = 'primary'
    WHERE r.release_id IS NOT NULL AND r.metadata_binding_id IS NOT NULL
    UNION ALL
    SELECT appearance.release_id, appearance.recording_id, appearance.id AS entry_id,
        appearance.title, appearance.artist, appearance.duration_ms,
        appearance.disc_number, appearance.track_number
    FROM catalog_appearances appearance;

CREATE VIEW artist_releases AS
    SELECT c.id AS artist_id, a.id AS release_id FROM artists c
    JOIN releases a ON a.library_id = c.library_id AND a.artist = c.name COLLATE BINARY
    WHERE EXISTS (SELECT 1 FROM source_metadata m WHERE m.release_id = a.id)
        OR EXISTS (SELECT 1 FROM provider_release_appearances appearance WHERE appearance.release_id = a.id)
    UNION ALL
    SELECT c.id, a.id FROM artists c
    CROSS JOIN source_artist_credits credit ON credit.artist_id = c.id
    CROSS JOIN source_metadata m ON m.source_id = credit.source_id
    CROSS JOIN releases a ON a.id = m.release_id AND a.library_id = c.library_id;
CREATE VIEW account_artist_releases AS
    SELECT user_id, artist_id, id AS release_id FROM account_releases a
    WHERE artist_id IS NOT NULL AND (
        EXISTS (SELECT 1 FROM source_metadata m WHERE m.release_id = a.id)
        OR EXISTS (SELECT 1 FROM provider_release_appearances appearance WHERE appearance.release_id = a.id))
    UNION ALL
    SELECT user_id, artist_id, release_id FROM account_source_credits
    UNION ALL
    SELECT credit.user_id, credit.artist_id, appearance.release_id
    FROM provider_release_appearances appearance
    JOIN account_recording_credits credit ON credit.recording_id = appearance.recording_id;
