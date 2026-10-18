PRAGMA defer_foreign_keys = ON;
DROP VIEW artist_releases;
DROP VIEW account_artist_releases;
CREATE INDEX recordings_release ON recordings(release_id, metadata_observation_id, id);
CREATE INDEX account_releases_release ON account_releases(id);
CREATE INDEX account_source_releases ON account_source_credits(release_id);
CREATE INDEX personal_release_lookup ON personal_release_metadata(release_id);

CREATE TABLE releases_next (
    id TEXT PRIMARY KEY,
    library_id TEXT NOT NULL REFERENCES libraries(id),
    title TEXT NOT NULL,
    artist TEXT,
    identity_key TEXT NOT NULL,
    UNIQUE(library_id, identity_key)
);
INSERT INTO releases_next(rowid,id,library_id,title,artist,identity_key)
SELECT rowid,id,library_id,title,artist,identity_key FROM releases;
DROP TABLE releases;
ALTER TABLE releases_next RENAME TO releases;
CREATE INDEX releases_artist ON releases(library_id, artist, id);
CREATE INDEX releases_order ON releases(title COLLATE NOCASE, id);
CREATE TRIGGER release_artist_insert AFTER INSERT ON releases WHEN new.artist IS NOT NULL BEGIN
    INSERT INTO artists VALUES (lower(hex(randomblob(16))), new.library_id, new.artist) ON CONFLICT(library_id, name) DO NOTHING;
END;
CREATE TRIGGER release_artist_update AFTER UPDATE OF artist, library_id ON releases WHEN new.artist IS NOT NULL BEGIN
    INSERT INTO artists VALUES (lower(hex(randomblob(16))), new.library_id, new.artist) ON CONFLICT(library_id, name) DO NOTHING;
END;
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

CREATE TABLE account_releases_next (
    user_id TEXT NOT NULL REFERENCES users(id),
    id TEXT NOT NULL REFERENCES releases(id),
    library_id TEXT NOT NULL REFERENCES libraries(id),
    title TEXT NOT NULL,
    artist TEXT,
    artist_id TEXT,
    scope TEXT GENERATED ALWAYS AS (hex(user_id)) STORED,
    PRIMARY KEY(user_id, id),
    FOREIGN KEY(user_id, artist_id) REFERENCES account_artists(user_id, id),
    CHECK((artist IS NULL) = (artist_id IS NULL))
);
INSERT INTO account_releases_next(rowid,user_id,id,library_id,title,artist,artist_id)
SELECT rowid,user_id,id,library_id,title,artist,artist_id FROM account_releases;
DROP TABLE account_releases;
ALTER TABLE account_releases_next RENAME TO account_releases;
CREATE INDEX account_releases_order ON account_releases(user_id, title COLLATE NOCASE, id);
CREATE INDEX account_releases_credit ON account_releases(user_id, artist_id, id);
CREATE INDEX account_releases_release ON account_releases(id);
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
    WHERE r.release_id IS NOT NULL AND r.metadata_binding_id IS NOT NULL;

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

CREATE TEMP TABLE hito_release_credit_fk_guard (value INTEGER NOT NULL CHECK(value = 0));
INSERT INTO hito_release_credit_fk_guard SELECT 1 FROM pragma_foreign_key_check;
DROP TABLE hito_release_credit_fk_guard;
PRAGMA defer_foreign_keys = OFF;
