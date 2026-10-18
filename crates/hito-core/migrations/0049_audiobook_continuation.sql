CREATE TABLE audiobook_continuations (
    user_id TEXT NOT NULL,
    audiobook_id TEXT NOT NULL,
    entry_id TEXT NOT NULL,
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    PRIMARY KEY(user_id,audiobook_id),
    FOREIGN KEY(audiobook_id,user_id) REFERENCES audiobooks(playlist_id,user_id),
    FOREIGN KEY(audiobook_id,entry_id) REFERENCES playlist_entries(playlist_id,id)
);
CREATE INDEX audiobook_continuations_chapter ON audiobook_continuations(audiobook_id,entry_id);
CREATE TRIGGER audiobook_continuation_scope BEFORE INSERT ON audiobook_continuations
WHEN NOT EXISTS (
    SELECT 1 FROM audiobooks book JOIN playlists playlist
        ON playlist.id=book.playlist_id AND playlist.user_id=book.user_id
    JOIN playlist_entries chapter ON chapter.playlist_id=book.playlist_id
    JOIN recordings recording ON recording.id=chapter.recording_id
    JOIN library_members access ON access.library_id=recording.library_id AND access.user_id=book.user_id
    WHERE book.playlist_id=new.audiobook_id AND book.user_id=new.user_id
        AND playlist.deleted=0 AND playlist.kind='local' AND chapter.deleted=0
        AND chapter.id=new.entry_id AND chapter.recording_id=new.recording_id
) BEGIN
    SELECT RAISE(ABORT, 'Continuation requires an owned audiobook chapter and recording access');
END;
CREATE TRIGGER audiobook_continuation_update_scope BEFORE UPDATE ON audiobook_continuations
WHEN old.user_id IS NOT new.user_id OR old.audiobook_id IS NOT new.audiobook_id OR NOT EXISTS (
    SELECT 1 FROM audiobooks book JOIN playlists playlist
        ON playlist.id=book.playlist_id AND playlist.user_id=book.user_id
    JOIN playlist_entries chapter ON chapter.playlist_id=book.playlist_id
    JOIN recordings recording ON recording.id=chapter.recording_id
    JOIN library_members access ON access.library_id=recording.library_id AND access.user_id=book.user_id
    WHERE book.playlist_id=new.audiobook_id AND book.user_id=new.user_id
        AND playlist.deleted=0 AND playlist.kind='local' AND chapter.deleted=0
        AND chapter.id=new.entry_id AND chapter.recording_id=new.recording_id
) BEGIN
    SELECT RAISE(ABORT, 'Continuation requires the same owner and an accessible audiobook chapter');
END;
