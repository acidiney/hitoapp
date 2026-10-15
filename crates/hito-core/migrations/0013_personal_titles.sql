CREATE TABLE personal_recording_titles (
    id INTEGER PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    override_title TEXT,
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    UNIQUE(user_id, recording_id)
);
CREATE INDEX personal_titles_recording ON personal_recording_titles(recording_id);
CREATE INDEX personal_titles_order ON personal_recording_titles(user_id, title COLLATE NOCASE, recording_id);
CREATE VIRTUAL TABLE personal_titles_fts USING fts5(title, artist, content='personal_recording_titles', content_rowid='id', tokenize='unicode61 remove_diacritics 2');
CREATE TRIGGER personal_title_insert AFTER INSERT ON personal_recording_titles BEGIN
    INSERT INTO personal_titles_fts(rowid, title, artist) VALUES(new.id, new.title, new.artist);
END;
CREATE TRIGGER personal_title_update AFTER UPDATE OF title, artist ON personal_recording_titles BEGIN
    INSERT INTO personal_titles_fts(personal_titles_fts, rowid, title, artist) VALUES('delete', old.id, old.title, old.artist);
    INSERT INTO personal_titles_fts(rowid, title, artist) VALUES(new.id, new.title, new.artist);
END;
CREATE TRIGGER personal_title_delete AFTER DELETE ON personal_recording_titles BEGIN
    INSERT INTO personal_titles_fts(personal_titles_fts, rowid, title, artist) VALUES('delete', old.id, old.title, old.artist);
END;
CREATE TRIGGER inherited_title_update AFTER UPDATE OF title, artist ON recordings BEGIN
    UPDATE personal_recording_titles SET title = COALESCE(override_title, new.title), artist = new.artist
    WHERE recording_id = new.id AND (title IS NOT COALESCE(override_title, new.title) OR artist IS NOT new.artist);
END;
