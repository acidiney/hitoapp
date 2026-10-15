CREATE TABLE metadata_observations (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    source_id TEXT NOT NULL REFERENCES sources(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    release_id TEXT REFERENCES releases(id),
    provider TEXT NOT NULL,
    origin TEXT NOT NULL CHECK(origin IN ('legacy_normalized', 'file_inspection')),
    observed_at INTEGER,
    file_checksum TEXT,
    inspection_version INTEGER NOT NULL,
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    album TEXT,
    album_artist TEXT,
    duration_ms INTEGER NOT NULL,
    disc_number INTEGER,
    track_number INTEGER,
    tags TEXT,
    UNIQUE(source_id, file_checksum, inspection_version)
);
CREATE INDEX observations_recording ON metadata_observations(recording_id, sequence DESC);
INSERT INTO metadata_observations(id, source_id, recording_id, release_id, provider, origin,
    file_checksum, inspection_version, title, artist, album, album_artist, duration_ms, disc_number, track_number)
SELECT lower(hex(randomblob(16))), m.source_id, s.recording_id, m.release_id, s.provider, 'legacy_normalized',
    f.file_checksum, m.inspection_version, m.title, m.artist, a.title, a.artist, m.duration_ms, m.disc_number, m.track_number
FROM source_metadata m JOIN sources s ON s.id = m.source_id
LEFT JOIN local_files f ON f.source_id = m.source_id LEFT JOIN releases a ON a.id = m.release_id;
ALTER TABLE source_metadata ADD COLUMN observation_id TEXT REFERENCES metadata_observations(id);
UPDATE source_metadata SET observation_id = (SELECT o.id FROM metadata_observations o WHERE o.source_id = source_metadata.source_id);
ALTER TABLE recordings ADD COLUMN metadata_observation_id TEXT REFERENCES metadata_observations(id);
UPDATE recordings SET metadata_observation_id = (SELECT m.observation_id FROM source_metadata m WHERE m.source_id = recordings.metadata_source_id);
