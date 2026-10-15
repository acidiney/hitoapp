CREATE TABLE music_folders (
    id TEXT PRIMARY KEY,
    library_id TEXT NOT NULL REFERENCES libraries(id),
    host_device TEXT NOT NULL,
    directory_id TEXT,
    relative_path TEXT NOT NULL DEFAULT '',
    root_path TEXT NOT NULL,
    name TEXT NOT NULL,
    connected INTEGER NOT NULL DEFAULT 1 CHECK(connected IN (0,1)),
    bound INTEGER NOT NULL DEFAULT 0 CHECK(bound IN (0,1)),
    revision INTEGER NOT NULL DEFAULT 1,
    generation INTEGER NOT NULL DEFAULT 1,
    last_scan_at INTEGER
);
CREATE UNIQUE INDEX music_folders_selection ON music_folders(library_id, host_device, directory_id, relative_path) WHERE directory_id IS NOT NULL;
CREATE INDEX music_folders_host ON music_folders(host_device, library_id, connected, bound);
INSERT INTO music_folders(id, library_id, host_device, root_path, name)
SELECT lower(hex(randomblob(16))), f.library_id, COALESCE(s.device_id, ''), f.scan_root, 'Music folder'
FROM local_files f JOIN sources s ON s.id = f.source_id
GROUP BY f.library_id, s.device_id, f.scan_root;

ALTER TABLE local_files RENAME TO previous_local_files;
CREATE TABLE local_files (
    source_id TEXT PRIMARY KEY REFERENCES sources(id),
    library_id TEXT NOT NULL REFERENCES libraries(id),
    folder_id TEXT NOT NULL REFERENCES music_folders(id),
    relative_path TEXT NOT NULL CHECK(json_valid(relative_path)),
    path TEXT NOT NULL,
    scan_root TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    original INTEGER NOT NULL DEFAULT 1,
    file_checksum TEXT,
    last_scan_attempt TEXT,
    inspection_scheme TEXT,
    audio_digest TEXT,
    UNIQUE(folder_id, relative_path)
);
INSERT INTO local_files(source_id, library_id, folder_id, relative_path, path, scan_root, size_bytes, original, file_checksum, last_scan_attempt, inspection_scheme, audio_digest)
SELECT f.source_id, f.library_id, m.id, json_quote(substr(f.path, length(rtrim(f.scan_root, '/\')) + 2)),
    f.path, f.scan_root, f.size_bytes, f.original, f.file_checksum, NULL, f.inspection_scheme, f.audio_digest
FROM previous_local_files f JOIN sources s ON s.id = f.source_id
JOIN music_folders m ON m.library_id = f.library_id AND m.host_device = COALESCE(s.device_id, '') AND m.root_path = f.scan_root;
DROP TABLE previous_local_files;
CREATE INDEX local_files_checksum ON local_files(library_id, file_checksum, source_id);
CREATE INDEX local_files_folder_scan ON local_files(folder_id, source_id);

ALTER TABLE jobs ADD COLUMN folder_id TEXT REFERENCES music_folders(id);
ALTER TABLE jobs ADD COLUMN folder_generation INTEGER;
ALTER TABLE jobs ADD COLUMN lease_token TEXT;
UPDATE jobs SET state = 'failed', error = 'Scan interrupted by storage upgrade. Scan your connected folders again.', lease_until = NULL
WHERE state IN ('queued','running');
DROP INDEX jobs_scan_active;
CREATE UNIQUE INDEX jobs_folder_active ON jobs(folder_id) WHERE folder_id IS NOT NULL AND state IN ('queued','running');
CREATE INDEX jobs_folder_history ON jobs(folder_id);
