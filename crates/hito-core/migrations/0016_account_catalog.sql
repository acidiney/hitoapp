CREATE TABLE account_artists (
    user_id TEXT NOT NULL REFERENCES users(id),
    id TEXT NOT NULL,
    library_id TEXT NOT NULL REFERENCES libraries(id),
    name TEXT NOT NULL COLLATE BINARY,
    scope TEXT GENERATED ALWAYS AS (hex(user_id)) STORED,
    PRIMARY KEY(user_id, id),
    UNIQUE(user_id, library_id, name)
);
CREATE INDEX account_artists_order ON account_artists(user_id, name COLLATE NOCASE, id);
INSERT INTO account_artists(user_id, id, library_id, name)
SELECT m.user_id, c.id, c.library_id, c.name FROM artists c JOIN library_members m ON m.library_id = c.library_id;
CREATE VIRTUAL TABLE account_artists_fts USING fts5(name, scope, content='account_artists', content_rowid='rowid', tokenize='unicode61 remove_diacritics 2');
INSERT INTO account_artists_fts(account_artists_fts) VALUES('rebuild');
CREATE TRIGGER account_artist_insert AFTER INSERT ON account_artists BEGIN
    INSERT INTO account_artists_fts(rowid, name, scope) VALUES(new.rowid, new.name, new.scope);
END;
CREATE TRIGGER account_artist_delete AFTER DELETE ON account_artists BEGIN
    INSERT INTO account_artists_fts(account_artists_fts, rowid, name, scope) VALUES('delete', old.rowid, old.name, old.scope);
END;
CREATE TABLE account_releases (
    user_id TEXT NOT NULL REFERENCES users(id),
    id TEXT NOT NULL REFERENCES releases(id),
    library_id TEXT NOT NULL REFERENCES libraries(id),
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    artist_id TEXT NOT NULL,
    scope TEXT GENERATED ALWAYS AS (hex(user_id)) STORED,
    PRIMARY KEY(user_id, id),
    FOREIGN KEY(user_id, artist_id) REFERENCES account_artists(user_id, id)
);
CREATE INDEX account_releases_order ON account_releases(user_id, title COLLATE NOCASE, id);
CREATE INDEX account_releases_credit ON account_releases(user_id, artist_id, id);
INSERT INTO account_releases(user_id, id, library_id, title, artist, artist_id)
SELECT m.user_id, a.id, a.library_id, a.title, a.artist, c.id
FROM releases a JOIN library_members m ON m.library_id = a.library_id
JOIN account_artists c ON c.user_id = m.user_id AND c.library_id = a.library_id AND c.name = a.artist COLLATE BINARY;
CREATE VIRTUAL TABLE account_releases_fts USING fts5(title, artist, scope, content='account_releases', content_rowid='rowid', tokenize='unicode61 remove_diacritics 2');
INSERT INTO account_releases_fts(account_releases_fts) VALUES('rebuild');
CREATE TRIGGER account_release_insert AFTER INSERT ON account_releases BEGIN
    INSERT INTO account_releases_fts(rowid, title, artist, scope) VALUES(new.rowid, new.title, new.artist, new.scope);
END;
CREATE TRIGGER account_release_update AFTER UPDATE OF title, artist ON account_releases
WHEN old.title IS NOT new.title OR old.artist IS NOT new.artist BEGIN
    INSERT INTO account_releases_fts(account_releases_fts, rowid, title, artist, scope) VALUES('delete', old.rowid, old.title, old.artist, old.scope);
    INSERT INTO account_releases_fts(rowid, title, artist, scope) VALUES(new.rowid, new.title, new.artist, new.scope);
END;
CREATE TRIGGER account_release_delete AFTER DELETE ON account_releases BEGIN
    INSERT INTO account_releases_fts(account_releases_fts, rowid, title, artist, scope) VALUES('delete', old.rowid, old.title, old.artist, old.scope);
END;
CREATE TABLE account_source_credits (
    user_id TEXT NOT NULL REFERENCES users(id),
    source_id TEXT NOT NULL REFERENCES source_metadata(source_id) ON DELETE CASCADE,
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    release_id TEXT NOT NULL REFERENCES releases(id),
    artist_id TEXT NOT NULL,
    PRIMARY KEY(user_id, source_id),
    FOREIGN KEY(user_id, artist_id) REFERENCES account_artists(user_id, id)
);
CREATE INDEX account_source_artists ON account_source_credits(user_id, artist_id, release_id);
CREATE INDEX account_source_recordings ON account_source_credits(recording_id, user_id, source_id);
INSERT INTO account_source_credits(user_id, source_id, recording_id, release_id, artist_id)
SELECT access.user_id, m.source_id, r.id, a.id, c.id
FROM source_metadata m JOIN sources s ON s.id = m.source_id JOIN recordings r ON r.id = s.recording_id
JOIN releases a ON a.id = m.release_id AND a.library_id = r.library_id
JOIN library_members access ON access.library_id = r.library_id
JOIN account_artists c ON c.user_id = access.user_id AND c.library_id = r.library_id AND c.name = m.artist COLLATE BINARY;
CREATE VIEW account_artist_releases AS
    SELECT user_id, artist_id, id AS release_id FROM account_releases a
    WHERE EXISTS(SELECT 1 FROM source_metadata m WHERE m.release_id = a.id)
    UNION ALL
    SELECT user_id, artist_id, release_id FROM account_source_credits;
