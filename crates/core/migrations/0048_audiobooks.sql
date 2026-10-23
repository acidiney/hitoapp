CREATE TABLE audiobooks (
    playlist_id TEXT PRIMARY KEY REFERENCES playlists(id),
    user_id TEXT NOT NULL REFERENCES users(id),
    UNIQUE(playlist_id,user_id)
);
CREATE INDEX audiobooks_account ON audiobooks(user_id,playlist_id);
CREATE TRIGGER audiobook_owner BEFORE INSERT ON audiobooks
WHEN NOT EXISTS (
    SELECT 1 FROM playlists p WHERE p.id=new.playlist_id AND p.user_id=new.user_id
        AND p.kind='local' AND p.deleted=0
) BEGIN
    SELECT RAISE(ABORT, 'Audiobooks require an owned editable playlist');
END;
CREATE TRIGGER audiobook_identity BEFORE UPDATE ON audiobooks
WHEN old.playlist_id IS NOT new.playlist_id OR old.user_id IS NOT new.user_id BEGIN
    SELECT RAISE(ABORT, 'Audiobook identity is immutable');
END;

CREATE TABLE audiobook_bookmarks (
    user_id TEXT NOT NULL REFERENCES users(id),
    id TEXT NOT NULL,
    audiobook_id TEXT NOT NULL,
    entry_id TEXT,
    recording_id TEXT REFERENCES recordings(id),
    position_ms INTEGER CHECK(position_ms IS NULL OR (typeof(position_ms)='integer' AND position_ms >= 0)),
    label TEXT CHECK(label IS NULL OR length(CAST(label AS BLOB)) BETWEEN 1 AND 256),
    deleted INTEGER NOT NULL DEFAULT 0 CHECK(deleted IN (0,1)),
    CHECK((deleted=1 AND entry_id IS NULL AND recording_id IS NULL AND position_ms IS NULL AND label IS NULL)
        OR (entry_id IS NOT NULL AND recording_id IS NOT NULL AND position_ms IS NOT NULL AND label IS NOT NULL)),
    PRIMARY KEY(user_id,id),
    FOREIGN KEY(audiobook_id,user_id) REFERENCES audiobooks(playlist_id,user_id),
    FOREIGN KEY(audiobook_id,entry_id) REFERENCES playlist_entries(playlist_id,id)
);
CREATE INDEX audiobook_bookmarks_account ON audiobook_bookmarks(user_id,audiobook_id,deleted,entry_id,position_ms,id);
CREATE INDEX audiobook_bookmarks_chapter ON audiobook_bookmarks(audiobook_id,entry_id);
CREATE TRIGGER audiobook_bookmark_scope BEFORE INSERT ON audiobook_bookmarks
WHEN NOT EXISTS (
    SELECT 1 FROM audiobooks b JOIN playlists p ON p.id=b.playlist_id AND p.user_id=b.user_id
    WHERE b.playlist_id=new.audiobook_id AND b.user_id=new.user_id AND p.deleted=0 AND p.kind='local'
) OR (new.deleted=0 AND NOT EXISTS (
    SELECT 1 FROM audiobooks b JOIN playlists p ON p.id=b.playlist_id AND p.user_id=b.user_id
    JOIN playlist_entries e ON e.playlist_id=p.id
    JOIN recordings r ON r.id=e.recording_id
    JOIN library_members access ON access.library_id=r.library_id AND access.user_id=b.user_id
    WHERE b.playlist_id=new.audiobook_id AND b.user_id=new.user_id AND p.deleted=0 AND p.kind='local'
        AND e.id=new.entry_id AND e.deleted=0 AND e.recording_id=new.recording_id
)) BEGIN
    SELECT RAISE(ABORT, 'Bookmarks require an owned audiobook chapter and recording access');
END;
CREATE TRIGGER audiobook_bookmark_limit BEFORE INSERT ON audiobook_bookmarks
WHEN new.deleted=0 AND (
    SELECT count(*) FROM audiobook_bookmarks b
    WHERE b.user_id=new.user_id AND b.audiobook_id=new.audiobook_id AND b.deleted=0
) >= 256 BEGIN
    SELECT RAISE(ABORT, 'Audiobook bookmark limit exceeded');
END;
CREATE TRIGGER audiobook_bookmark_identity BEFORE UPDATE ON audiobook_bookmarks
WHEN old.user_id IS NOT new.user_id OR old.id IS NOT new.id
    OR old.audiobook_id IS NOT new.audiobook_id OR old.entry_id IS NOT new.entry_id
    OR old.recording_id IS NOT new.recording_id OR old.position_ms IS NOT new.position_ms BEGIN
    SELECT RAISE(ABORT, 'Bookmark identity is immutable');
END;
