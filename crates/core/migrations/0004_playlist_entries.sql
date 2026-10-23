CREATE TABLE playlist_entries (
    playlist_id TEXT NOT NULL REFERENCES playlists(id),
    id TEXT NOT NULL,
    recording_id TEXT REFERENCES recordings(id),
    position INTEGER NOT NULL DEFAULT 0,
    deleted INTEGER NOT NULL DEFAULT 0 CHECK(deleted IN (0,1)),
    PRIMARY KEY(playlist_id,id)
);
CREATE INDEX playlist_order ON playlist_entries(playlist_id,deleted,position,id);
