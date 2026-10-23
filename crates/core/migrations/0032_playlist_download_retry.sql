CREATE TABLE download_terminal_failures (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    intent_id TEXT NOT NULL UNIQUE REFERENCES download_intents(id)
);
INSERT INTO download_terminal_failures(intent_id)
SELECT id FROM download_intents WHERE state='failed' ORDER BY created_at,id;
CREATE TRIGGER download_terminal_failure_insert AFTER INSERT ON download_intents
WHEN new.state='failed' BEGIN
    INSERT INTO download_terminal_failures(intent_id) VALUES(new.id);
END;
CREATE TRIGGER download_terminal_failure_update AFTER UPDATE OF state ON download_intents
WHEN new.state='failed' AND old.state!='failed' BEGIN
    INSERT INTO download_terminal_failures(intent_id) VALUES(new.id);
END;
CREATE TRIGGER download_terminal_failure_immutable BEFORE UPDATE ON download_terminal_failures BEGIN
    SELECT RAISE(ABORT, 'Download terminal failures are immutable');
END;
CREATE TRIGGER download_terminal_failure_retained BEFORE DELETE ON download_terminal_failures BEGIN
    SELECT RAISE(ABORT, 'Download terminal failures must be retained');
END;

CREATE TABLE download_playlist_retry_work (
    retention_id TEXT PRIMARY KEY,
    host_device TEXT NOT NULL,
    request_id TEXT NOT NULL,
    playlist_revision INTEGER NOT NULL CHECK(playlist_revision>0),
    failure_cutoff INTEGER NOT NULL CHECK(failure_cutoff>=0),
    cursor TEXT,
    next_attempt_at INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY(retention_id,host_device) REFERENCES download_retentions(id,host_device)
);
CREATE INDEX download_playlist_retry_due ON download_playlist_retry_work(host_device,next_attempt_at,retention_id);
CREATE INDEX playlist_download_retry_scan ON playlist_entries(playlist_id,deleted,recording_id);
