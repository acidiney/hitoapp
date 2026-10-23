PRAGMA defer_foreign_keys = ON;
CREATE INDEX playlist_import_receipt_publication ON playlist_import_receipts(publication_sequence);
CREATE INDEX playlist_follow_receipt_publication ON playlist_follow_receipts(publication_sequence);
CREATE TEMP TABLE hito_metadata_sync_sequences AS
SELECT rowid AS original_rowid,name,seq FROM sqlite_sequence;
CREATE TABLE sync_events_next (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL REFERENCES users(id),
    origin TEXT NOT NULL CHECK(origin IN ('client_operation','playlist_import','playlist_follow','playlist_metadata')),
    device_id TEXT,
    event_id TEXT NOT NULL,
    payload TEXT NOT NULL,
    UNIQUE(user_id,origin,event_id),
    CHECK((origin = 'client_operation' AND device_id IS NOT NULL)
        OR (origin IN ('playlist_import','playlist_follow','playlist_metadata') AND device_id IS NULL))
);
INSERT INTO sync_events_next(sequence,user_id,origin,device_id,event_id,payload)
SELECT sequence,user_id,origin,device_id,event_id,payload FROM sync_events;
DROP TABLE sync_events;
ALTER TABLE sync_events_next RENAME TO sync_events;
DELETE FROM sqlite_sequence;
INSERT INTO sqlite_sequence(rowid,name,seq)
SELECT original_rowid,name,seq FROM hito_metadata_sync_sequences ORDER BY original_rowid;
DROP TABLE hito_metadata_sync_sequences;
CREATE INDEX sync_events_account_sequence ON sync_events(user_id,sequence);

CREATE TABLE playlist_metadata_refresh_receipts (
    user_id TEXT NOT NULL REFERENCES users(id),
    request_id TEXT NOT NULL,
    request_digest TEXT NOT NULL CHECK(length(request_digest) = 64),
    playlist_id TEXT NOT NULL REFERENCES playlist_imports(playlist_id),
    updated_recordings INTEGER NOT NULL CHECK(updated_recordings BETWEEN 0 AND 10000),
    unchanged_recordings INTEGER NOT NULL CHECK(unchanged_recordings BETWEEN 0 AND 10000),
    unavailable_recordings INTEGER NOT NULL CHECK(unavailable_recordings BETWEEN 0 AND 10000),
    publication_sequence INTEGER REFERENCES sync_events(sequence),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(user_id,request_id),
    CHECK(updated_recordings + unchanged_recordings + unavailable_recordings <= 10000),
    CHECK((updated_recordings = 0) = (publication_sequence IS NULL))
);
CREATE TRIGGER playlist_metadata_receipt_immutable BEFORE UPDATE ON playlist_metadata_refresh_receipts BEGIN
    SELECT RAISE(ABORT, 'Playlist metadata receipts are immutable');
END;
CREATE TRIGGER playlist_metadata_receipt_retained BEFORE DELETE ON playlist_metadata_refresh_receipts BEGIN
    SELECT RAISE(ABORT, 'Playlist metadata receipts must be retained');
END;

CREATE TEMP TABLE hito_metadata_refresh_fk_guard (value INTEGER NOT NULL CHECK(value = 0));
INSERT INTO hito_metadata_refresh_fk_guard SELECT 1 FROM pragma_foreign_key_check;
DROP TABLE hito_metadata_refresh_fk_guard;
PRAGMA defer_foreign_keys = OFF;
