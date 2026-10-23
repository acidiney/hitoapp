CREATE TABLE releases (
    id TEXT PRIMARY KEY,
    library_id TEXT NOT NULL REFERENCES libraries(id),
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    identity_key TEXT NOT NULL,
    UNIQUE(library_id, identity_key)
);
CREATE TABLE recordings (
    id TEXT PRIMARY KEY,
    library_id TEXT NOT NULL REFERENCES libraries(id),
    release_id TEXT REFERENCES releases(id),
    title TEXT NOT NULL,
    artist TEXT NOT NULL,
    duration_ms INTEGER NOT NULL DEFAULT 0 CHECK(duration_ms >= 0),
    content_hash TEXT NOT NULL,
    provenance TEXT NOT NULL DEFAULT 'file_tags',
    UNIQUE(library_id,content_hash)
);
CREATE INDEX recordings_library ON recordings(library_id, title, id);
CREATE VIRTUAL TABLE recordings_fts USING fts5(title, artist, content='recordings', content_rowid='rowid', tokenize='unicode61 remove_diacritics 2');
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
CREATE TABLE sources (
    id TEXT PRIMARY KEY,
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    provider TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    device_id TEXT,
    ready INTEGER NOT NULL DEFAULT 1 CHECK(ready IN (0,1)),
    UNIQUE(provider,provider_id,recording_id)
);
CREATE INDEX sources_recording ON sources(recording_id);
CREATE TABLE local_files (
    source_id TEXT PRIMARY KEY REFERENCES sources(id),
    library_id TEXT NOT NULL REFERENCES libraries(id),
    path TEXT NOT NULL,
    scan_root TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    original INTEGER NOT NULL DEFAULT 1,
    UNIQUE(library_id,path)
);
CREATE TABLE jobs (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    library_id TEXT NOT NULL REFERENCES libraries(id),
    kind TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'queued' CHECK(state IN ('queued','running','complete','failed')),
    progress INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    lease_until INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX jobs_pending ON jobs(state,lease_until,created_at);
CREATE UNIQUE INDEX jobs_scan_active ON jobs(library_id,kind) WHERE state IN ('queued','running');
