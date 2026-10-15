CREATE TABLE artists (
    id TEXT PRIMARY KEY,
    library_id TEXT NOT NULL REFERENCES libraries(id),
    name TEXT NOT NULL COLLATE BINARY,
    UNIQUE(library_id, name)
);
INSERT INTO artists SELECT lower(hex(randomblob(16))), library_id, artist FROM (
    SELECT library_id, artist FROM recordings UNION SELECT library_id, artist FROM releases
    UNION SELECT r.library_id, m.artist FROM source_metadata m JOIN sources s ON s.id = m.source_id JOIN recordings r ON r.id = s.recording_id
);
CREATE TRIGGER recording_artist_insert AFTER INSERT ON recordings BEGIN
    INSERT INTO artists VALUES (lower(hex(randomblob(16))), new.library_id, new.artist) ON CONFLICT(library_id, name) DO NOTHING;
END;
CREATE TRIGGER recording_artist_update AFTER UPDATE OF artist, library_id ON recordings BEGIN
    INSERT INTO artists VALUES (lower(hex(randomblob(16))), new.library_id, new.artist) ON CONFLICT(library_id, name) DO NOTHING;
END;
CREATE TRIGGER release_artist_insert AFTER INSERT ON releases BEGIN
    INSERT INTO artists VALUES (lower(hex(randomblob(16))), new.library_id, new.artist) ON CONFLICT(library_id, name) DO NOTHING;
END;
CREATE TRIGGER release_artist_update AFTER UPDATE OF artist, library_id ON releases BEGIN
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
CREATE INDEX releases_artist ON releases(library_id, artist, id);
CREATE INDEX source_metadata_artist ON source_metadata(artist, release_id, source_id);
CREATE INDEX artists_order ON artists(name COLLATE NOCASE, id);
CREATE INDEX releases_order ON releases(title COLLATE NOCASE, id);
CREATE VIEW artist_releases AS
    SELECT c.id AS artist_id, a.id AS release_id FROM artists c
    JOIN releases a ON a.library_id = c.library_id AND a.artist = c.name COLLATE BINARY
    WHERE EXISTS (SELECT 1 FROM source_metadata m WHERE m.release_id = a.id)
    UNION
    SELECT c.id, a.id FROM artists c
    JOIN source_metadata m ON m.artist = c.name COLLATE BINARY
    JOIN releases a ON a.id = m.release_id AND a.library_id = c.library_id;
