CREATE VIRTUAL TABLE artists_fts USING fts5(name, content='artists', content_rowid='rowid', tokenize='unicode61 remove_diacritics 2');
INSERT INTO artists_fts(artists_fts) VALUES('rebuild');
CREATE TRIGGER artist_search_insert AFTER INSERT ON artists BEGIN
    INSERT INTO artists_fts(rowid, name) VALUES(new.rowid, new.name);
END;
CREATE TRIGGER artist_search_delete AFTER DELETE ON artists BEGIN
    INSERT INTO artists_fts(artists_fts, rowid, name) VALUES('delete', old.rowid, old.name);
END;
CREATE TRIGGER artist_search_update AFTER UPDATE OF name ON artists WHEN old.name IS NOT new.name BEGIN
    INSERT INTO artists_fts(artists_fts, rowid, name) VALUES('delete', old.rowid, old.name);
    INSERT INTO artists_fts(rowid, name) VALUES(new.rowid, new.name);
END;
CREATE VIRTUAL TABLE releases_fts USING fts5(title, artist, content='releases', content_rowid='rowid', tokenize='unicode61 remove_diacritics 2');
INSERT INTO releases_fts(releases_fts) VALUES('rebuild');
CREATE TRIGGER release_search_insert AFTER INSERT ON releases BEGIN
    INSERT INTO releases_fts(rowid, title, artist) VALUES(new.rowid, new.title, new.artist);
END;
CREATE TRIGGER release_search_delete AFTER DELETE ON releases BEGIN
    INSERT INTO releases_fts(releases_fts, rowid, title, artist) VALUES('delete', old.rowid, old.title, old.artist);
END;
CREATE TRIGGER release_search_update AFTER UPDATE OF title, artist ON releases WHEN old.title IS NOT new.title OR old.artist IS NOT new.artist BEGIN
    INSERT INTO releases_fts(releases_fts, rowid, title, artist) VALUES('delete', old.rowid, old.title, old.artist);
    INSERT INTO releases_fts(rowid, title, artist) VALUES(new.rowid, new.title, new.artist);
END;
DROP VIEW artist_releases;
CREATE VIEW artist_releases AS
    SELECT c.id AS artist_id, a.id AS release_id FROM artists c
    JOIN releases a ON a.library_id = c.library_id AND a.artist = c.name COLLATE BINARY
    WHERE EXISTS (SELECT 1 FROM source_metadata m WHERE m.release_id = a.id)
    UNION ALL
    SELECT c.id, a.id FROM artists c
    CROSS JOIN releases a ON a.library_id = c.library_id
    CROSS JOIN source_metadata m ON m.release_id = a.id AND m.artist = c.name COLLATE BINARY;
