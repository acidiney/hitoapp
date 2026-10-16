CREATE TEMP TABLE hito_sync_high_water AS
SELECT COALESCE(MAX(seq), 0) AS sequence FROM sqlite_sequence WHERE name = 'sync_operations';

CREATE TABLE sync_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL REFERENCES users(id),
    origin TEXT NOT NULL CHECK(origin IN ('client_operation', 'playlist_import')),
    device_id TEXT,
    event_id TEXT NOT NULL,
    payload TEXT NOT NULL,
    UNIQUE(user_id, origin, event_id),
    CHECK((origin = 'client_operation' AND device_id IS NOT NULL)
       OR (origin = 'playlist_import' AND device_id IS NULL))
);
INSERT INTO sync_events(sequence, user_id, origin, device_id, event_id, payload)
SELECT sequence, user_id, 'client_operation', device_id, operation_id, payload FROM sync_operations;
INSERT INTO sqlite_sequence(name, seq)
SELECT 'sync_events', sequence FROM hito_sync_high_water
WHERE NOT EXISTS(SELECT 1 FROM sqlite_sequence WHERE name = 'sync_events');
UPDATE sqlite_sequence SET seq = MAX(seq, (SELECT sequence FROM hito_sync_high_water))
WHERE name = 'sync_events';
DROP TABLE sync_operations;
DROP TABLE hito_sync_high_water;
CREATE INDEX sync_events_account_sequence ON sync_events(user_id, sequence);

CREATE TABLE playlist_imports (
    playlist_id TEXT PRIMARY KEY REFERENCES playlists(id),
    installation_id TEXT NOT NULL REFERENCES plugin_installations(id),
    installation_revision INTEGER NOT NULL,
    service_id TEXT NOT NULL REFERENCES plugin_services(id),
    service_revision INTEGER NOT NULL,
    provider TEXT NOT NULL,
    provider_playlist_id TEXT NOT NULL,
    snapshot TEXT NOT NULL,
    attribution TEXT NOT NULL,
    imported_at INTEGER NOT NULL
);
CREATE TABLE playlist_import_entries (
    playlist_id TEXT NOT NULL REFERENCES playlist_imports(playlist_id),
    entry_id TEXT NOT NULL,
    occurrence_id TEXT,
    provider_recording_id TEXT NOT NULL,
    PRIMARY KEY(playlist_id, entry_id),
    FOREIGN KEY(playlist_id, entry_id) REFERENCES playlist_entries(playlist_id, id)
);
CREATE TABLE playlist_import_receipts (
    user_id TEXT NOT NULL REFERENCES users(id),
    request_id TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    playlist_id TEXT NOT NULL REFERENCES playlists(id),
    publication_sequence INTEGER NOT NULL REFERENCES sync_events(sequence),
    PRIMARY KEY(user_id, request_id)
);
