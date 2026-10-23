DROP TRIGGER inherited_title_update;
CREATE TABLE personal_recording_metadata (
    id INTEGER PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    override_title TEXT,
    override_artist TEXT,
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    title_operation_id TEXT,
    artist_operation_id TEXT,
    scope TEXT GENERATED ALWAYS AS (hex(user_id)) STORED,
    UNIQUE(user_id, recording_id),
    CHECK(title_operation_id IS NOT NULL OR artist_operation_id IS NOT NULL),
    CHECK(override_title IS NULL OR title_operation_id IS NOT NULL),
    CHECK(override_artist IS NULL OR artist_operation_id IS NOT NULL)
);
INSERT INTO personal_recording_metadata(id, user_id, recording_id, override_title, title, artist, title_operation_id)
SELECT id, user_id, recording_id, override_title, title, artist, operation_id FROM personal_recording_titles;
DROP TABLE personal_titles_fts;
DROP TABLE personal_recording_titles;
CREATE INDEX personal_metadata_recording ON personal_recording_metadata(recording_id);
CREATE INDEX personal_metadata_order ON personal_recording_metadata(user_id, title COLLATE NOCASE, recording_id);
CREATE VIRTUAL TABLE personal_recordings_fts USING fts5(title, artist, scope, content='personal_recording_metadata', content_rowid='id', tokenize='unicode61 remove_diacritics 2');
INSERT INTO personal_recordings_fts(personal_recordings_fts) VALUES('rebuild');
CREATE TRIGGER personal_metadata_insert AFTER INSERT ON personal_recording_metadata BEGIN
    INSERT INTO personal_recordings_fts(rowid, title, artist, scope) VALUES(new.id, new.title, new.artist, new.scope);
END;
CREATE TRIGGER personal_metadata_update AFTER UPDATE OF title, artist ON personal_recording_metadata
WHEN old.title IS NOT new.title OR old.artist IS NOT new.artist BEGIN
    INSERT INTO personal_recordings_fts(personal_recordings_fts, rowid, title, artist, scope) VALUES('delete', old.id, old.title, old.artist, old.scope);
    INSERT INTO personal_recordings_fts(rowid, title, artist, scope) VALUES(new.id, new.title, new.artist, new.scope);
END;
CREATE TRIGGER personal_metadata_delete AFTER DELETE ON personal_recording_metadata BEGIN
    INSERT INTO personal_recordings_fts(personal_recordings_fts, rowid, title, artist, scope) VALUES('delete', old.id, old.title, old.artist, old.scope);
END;
CREATE TRIGGER inherited_metadata_update AFTER UPDATE OF title, artist ON recordings BEGIN
    UPDATE personal_recording_metadata SET title = COALESCE(override_title, new.title), artist = COALESCE(override_artist, new.artist)
    WHERE recording_id = new.id AND (title IS NOT COALESCE(override_title, new.title) OR artist IS NOT COALESCE(override_artist, new.artist));
END;
