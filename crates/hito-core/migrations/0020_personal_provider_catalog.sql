ALTER TABLE libraries ADD COLUMN owner_id TEXT REFERENCES users(id);
CREATE INDEX account_source_lookup ON account_source_credits(source_id);
CREATE UNIQUE INDEX libraries_owner ON libraries(owner_id) WHERE owner_id IS NOT NULL;
CREATE TRIGGER library_owner_immutable BEFORE UPDATE OF owner_id ON libraries
WHEN old.owner_id IS NOT new.owner_id BEGIN
    SELECT RAISE(ABORT, 'Library ownership is immutable');
END;
CREATE TRIGGER personal_membership_insert BEFORE INSERT ON library_members
WHEN EXISTS(SELECT 1 FROM libraries WHERE id = new.library_id AND owner_id IS NOT NULL AND owner_id != new.user_id) BEGIN
    SELECT RAISE(ABORT, 'Personal libraries cannot be shared');
END;
CREATE TRIGGER personal_membership_update BEFORE UPDATE ON library_members
WHEN EXISTS(SELECT 1 FROM libraries WHERE id = old.library_id AND owner_id IS NOT NULL)
    OR EXISTS(SELECT 1 FROM libraries WHERE id = new.library_id AND owner_id IS NOT NULL) BEGIN
    SELECT RAISE(ABORT, 'Personal membership is immutable');
END;
CREATE TRIGGER personal_membership_delete BEFORE DELETE ON library_members
WHEN EXISTS(SELECT 1 FROM libraries WHERE id = old.library_id AND owner_id IS NOT NULL) BEGIN
    SELECT RAISE(ABORT, 'Personal membership is immutable');
END;
CREATE TRIGGER personal_folder_insert BEFORE INSERT ON music_folders
WHEN EXISTS(SELECT 1 FROM libraries WHERE id = new.library_id AND owner_id IS NOT NULL) BEGIN
    SELECT RAISE(ABORT, 'Personal provider libraries cannot contain music folders');
END;
CREATE TRIGGER personal_folder_update BEFORE UPDATE OF library_id ON music_folders
WHEN EXISTS(SELECT 1 FROM libraries WHERE id = new.library_id AND owner_id IS NOT NULL) BEGIN
    SELECT RAISE(ABORT, 'Personal provider libraries cannot contain music folders');
END;

CREATE TABLE provider_items (
    installation_id TEXT NOT NULL REFERENCES plugin_installations(id),
    provider_recording_id TEXT NOT NULL,
    recording_id TEXT NOT NULL UNIQUE REFERENCES recordings(id),
    PRIMARY KEY(installation_id,provider_recording_id),
    UNIQUE(installation_id,recording_id)
);
CREATE TRIGGER provider_item_owner BEFORE INSERT ON provider_items
WHEN NOT EXISTS(SELECT 1 FROM plugin_installations p JOIN libraries l ON l.owner_id = p.user_id
    JOIN recordings r ON r.library_id = l.id WHERE p.id = new.installation_id AND r.id = new.recording_id) BEGIN
    SELECT RAISE(ABORT, 'Provider recordings require their account library');
END;
CREATE TRIGGER provider_item_immutable BEFORE UPDATE ON provider_items BEGIN
    SELECT RAISE(ABORT, 'Provider item identity is immutable');
END;
CREATE TRIGGER provider_recording_library BEFORE UPDATE OF library_id ON recordings
WHEN old.library_id != new.library_id AND EXISTS(SELECT 1 FROM provider_items WHERE recording_id = old.id) BEGIN
    SELECT RAISE(ABORT, 'Provider recording ownership is immutable');
END;
CREATE UNIQUE INDEX sources_identity_recording ON sources(id,recording_id);
CREATE TABLE provider_sources (
    source_id TEXT PRIMARY KEY,
    installation_id TEXT NOT NULL,
    recording_id TEXT NOT NULL,
    provider_source_id TEXT NOT NULL,
    label TEXT NOT NULL,
    UNIQUE(installation_id,provider_source_id),
    FOREIGN KEY(installation_id,recording_id) REFERENCES provider_items(installation_id,recording_id),
    FOREIGN KEY(source_id,recording_id) REFERENCES sources(id,recording_id)
);
CREATE INDEX provider_sources_recording ON provider_sources(installation_id,recording_id,source_id);
