CREATE TABLE library_access_grants (
    library_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    id TEXT NOT NULL DEFAULT (lower(hex(randomblob(16)))),
    PRIMARY KEY(library_id, user_id),
    FOREIGN KEY(library_id, user_id) REFERENCES library_members(library_id, user_id) ON DELETE CASCADE ON UPDATE CASCADE
);
INSERT INTO library_access_grants(library_id, user_id) SELECT library_id, user_id FROM library_members;
CREATE TRIGGER library_access_grant_insert AFTER INSERT ON library_members BEGIN
    INSERT INTO library_access_grants(library_id, user_id) VALUES(new.library_id, new.user_id);
END;

ALTER TABLE plugin_installations ADD COLUMN audio_generation INTEGER NOT NULL DEFAULT 0 CHECK(audio_generation >= 0);
CREATE TRIGGER plugin_audio_capability_revoked AFTER UPDATE OF manifest ON plugin_installations
WHEN EXISTS(SELECT 1 FROM json_each(old.manifest, '$.capabilities') WHERE value = 'sources')
    AND NOT EXISTS(SELECT 1 FROM json_each(new.manifest, '$.capabilities') WHERE value = 'sources') BEGIN
    UPDATE plugin_installations SET audio_generation = audio_generation + 1 WHERE id = new.id;
END;
