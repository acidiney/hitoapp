DROP TRIGGER source_artist_insert;
DROP TRIGGER source_artist_update;

CREATE TRIGGER source_artist_insert AFTER INSERT ON source_metadata BEGIN
    INSERT INTO artists SELECT lower(hex(randomblob(16))), r.library_id, new.artist
    FROM sources s INDEXED BY sources_identity_recording
    CROSS JOIN recordings r INDEXED BY sqlite_autoindex_recordings_1 ON r.id = s.recording_id
    WHERE s.id = new.source_id ON CONFLICT(library_id, name) DO NOTHING;
END;
CREATE TRIGGER source_artist_update AFTER UPDATE OF artist, source_id ON source_metadata BEGIN
    INSERT INTO artists SELECT lower(hex(randomblob(16))), r.library_id, new.artist
    FROM sources s INDEXED BY sources_identity_recording
    CROSS JOIN recordings r INDEXED BY sqlite_autoindex_recordings_1 ON r.id = s.recording_id
    WHERE s.id = new.source_id ON CONFLICT(library_id, name) DO NOTHING;
END;
