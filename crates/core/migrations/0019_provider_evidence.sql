PRAGMA defer_foreign_keys = ON;
CREATE INDEX likes_recording ON likes(recording_id);
CREATE INDEX listening_progress_recording ON listening_progress(recording_id);
CREATE INDEX playlist_entries_recording ON playlist_entries(recording_id);
DROP TRIGGER source_artist_insert;
DROP TRIGGER source_artist_update;

CREATE TABLE recordings_next (
    id TEXT PRIMARY KEY,
    library_id TEXT NOT NULL REFERENCES libraries(id),
    release_id TEXT REFERENCES releases(id),
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    duration_ms INTEGER NOT NULL DEFAULT 0 CHECK(duration_ms >= 0),
    initial_file_checksum TEXT,
    provenance TEXT NOT NULL DEFAULT 'file_tags',
    metadata_source_id TEXT REFERENCES sources(id),
    metadata_observation_id TEXT REFERENCES metadata_observations(id),
    UNIQUE(library_id, initial_file_checksum)
);
INSERT INTO recordings_next(rowid,id,library_id,release_id,title,artist,duration_ms,initial_file_checksum,provenance,metadata_source_id,metadata_observation_id)
    SELECT rowid,id,library_id,release_id,title,artist,duration_ms,initial_file_checksum,provenance,metadata_source_id,metadata_observation_id FROM recordings;
DROP TABLE recordings;
ALTER TABLE recordings_next RENAME TO recordings;
CREATE INDEX recordings_observation ON recordings(metadata_observation_id);
CREATE INDEX source_metadata_observation ON source_metadata(observation_id);
CREATE INDEX recordings_library ON recordings(library_id,title,id);
CREATE INDEX recordings_catalog_order ON recordings(title COLLATE NOCASE,id,library_id);

CREATE TEMP TABLE hito_observation_sequence AS SELECT seq FROM sqlite_sequence WHERE name = 'metadata_observations';
CREATE TABLE metadata_observations_next (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    source_id TEXT REFERENCES sources(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    release_id TEXT REFERENCES releases(id),
    provider TEXT NOT NULL,
    origin TEXT NOT NULL CHECK(origin IN ('legacy_normalized','file_inspection','provider_response')),
    observed_at INTEGER,
    file_checksum TEXT,
    inspection_version INTEGER,
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    album TEXT,
    album_artist TEXT,
    duration_ms INTEGER NOT NULL,
    disc_number INTEGER,
    track_number INTEGER,
    tags TEXT,
    plugin_installation_id TEXT REFERENCES plugin_installations(id),
    provider_recording_id TEXT,
    provider_revision TEXT,
    provider_metadata TEXT,
    provider_digest TEXT,
    UNIQUE(plugin_installation_id,provider_recording_id,provider_digest),
    UNIQUE(source_id,file_checksum,inspection_version)
);
INSERT INTO metadata_observations_next(sequence,id,source_id,recording_id,release_id,provider,origin,observed_at,file_checksum,inspection_version,title,artist,album,album_artist,duration_ms,disc_number,track_number,tags)
    SELECT sequence,id,source_id,recording_id,release_id,provider,origin,observed_at,file_checksum,inspection_version,title,artist,album,album_artist,duration_ms,disc_number,track_number,tags FROM metadata_observations;
DROP TABLE metadata_observations;
ALTER TABLE metadata_observations_next RENAME TO metadata_observations;
UPDATE sqlite_sequence SET seq = max(seq, COALESCE((SELECT seq FROM hito_observation_sequence),0)) WHERE name = 'metadata_observations';
DROP TABLE hito_observation_sequence;
CREATE INDEX observations_recording ON metadata_observations(recording_id,sequence DESC);
CREATE INDEX observations_release ON metadata_observations(release_id,sequence DESC);

CREATE TRIGGER recording_insert AFTER INSERT ON recordings BEGIN
  INSERT INTO recordings_fts(rowid,title,artist) VALUES(new.rowid,new.title,new.artist);
END;
CREATE TRIGGER recording_delete AFTER DELETE ON recordings BEGIN
  INSERT INTO recordings_fts(recordings_fts,rowid,title,artist) VALUES('delete',old.rowid,old.title,old.artist);
END;
CREATE TRIGGER recording_update AFTER UPDATE ON recordings BEGIN
  INSERT INTO recordings_fts(recordings_fts,rowid,title,artist) VALUES('delete',old.rowid,old.title,old.artist);
  INSERT INTO recordings_fts(rowid,title,artist) VALUES(new.rowid,new.title,new.artist);
END;
CREATE TRIGGER recording_artist_insert AFTER INSERT ON recordings BEGIN
    INSERT INTO artists VALUES (lower(hex(randomblob(16))), new.library_id, new.artist) ON CONFLICT(library_id, name) DO NOTHING;
END;
CREATE TRIGGER recording_artist_update AFTER UPDATE OF artist, library_id ON recordings BEGIN
    INSERT INTO artists VALUES (lower(hex(randomblob(16))), new.library_id, new.artist) ON CONFLICT(library_id, name) DO NOTHING;
END;
CREATE TRIGGER source_artist_insert AFTER INSERT ON source_metadata BEGIN
    INSERT INTO artists SELECT lower(hex(randomblob(16))), r.library_id, new.artist
    FROM sources s JOIN recordings r ON r.id = s.recording_id WHERE s.id = new.source_id ON CONFLICT(library_id, name) DO NOTHING;
END;
CREATE TRIGGER source_artist_update AFTER UPDATE OF artist, source_id ON source_metadata BEGIN
    INSERT INTO artists SELECT lower(hex(randomblob(16))), r.library_id, new.artist
    FROM sources s JOIN recordings r ON r.id = s.recording_id WHERE s.id = new.source_id ON CONFLICT(library_id, name) DO NOTHING;
END;
CREATE TRIGGER inherited_metadata_update AFTER UPDATE OF title, artist ON recordings BEGIN
    UPDATE personal_recording_metadata SET title = COALESCE(override_title, new.title), artist = COALESCE(override_artist, new.artist)
    WHERE recording_id = new.id AND (title IS NOT COALESCE(override_title, new.title) OR artist IS NOT COALESCE(override_artist, new.artist));
END;

CREATE TEMP TABLE hito_provider_fk_guard (value INTEGER NOT NULL CHECK(value = 0));
INSERT INTO hito_provider_fk_guard SELECT 1 FROM pragma_foreign_key_check;
DROP TABLE hito_provider_fk_guard;
PRAGMA defer_foreign_keys = OFF;
