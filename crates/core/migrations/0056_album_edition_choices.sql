CREATE TABLE album_edition_choices (
    user_id TEXT NOT NULL REFERENCES users(id),
    release_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision>0),
    recording_id TEXT REFERENCES recordings(id),
    association_id TEXT REFERENCES recording_artist_associations(id),
    provider_kind TEXT CHECK(provider_kind IN ('builtin_musicbrainz','http_plugin')),
    plugin_installation_id TEXT REFERENCES plugin_installations(id),
    release_group_id TEXT CHECK(length(CAST(release_group_id AS BLOB)) BETWEEN 1 AND 256),
    edition_id TEXT CHECK(length(CAST(edition_id AS BLOB)) BETWEEN 1 AND 256),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(user_id,release_id),
    CHECK((recording_id IS NULL AND association_id IS NULL AND provider_kind IS NULL AND plugin_installation_id IS NULL
            AND release_group_id IS NULL AND edition_id IS NULL)
        OR (recording_id IS NOT NULL AND association_id IS NOT NULL AND provider_kind IS NOT NULL
            AND release_group_id IS NOT NULL AND edition_id IS NOT NULL
            AND ((provider_kind='builtin_musicbrainz' AND plugin_installation_id IS NULL)
                OR (provider_kind='http_plugin' AND plugin_installation_id IS NOT NULL))))
);
CREATE TRIGGER album_edition_choice_scope_insert BEFORE INSERT ON album_edition_choices
WHEN new.association_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM recording_artist_associations association
    WHERE association.id=new.association_id AND association.user_id=new.user_id
        AND association.recording_id=new.recording_id AND association.provider_kind=new.provider_kind
        AND association.plugin_installation_id IS new.plugin_installation_id
) BEGIN
    SELECT RAISE(ABORT,'Album edition choices require the account''s own artist association');
END;
CREATE TRIGGER album_edition_choice_scope_update BEFORE UPDATE ON album_edition_choices
WHEN new.user_id IS NOT old.user_id OR new.release_id IS NOT old.release_id OR (new.association_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM recording_artist_associations association
    WHERE association.id=new.association_id AND association.user_id=new.user_id
        AND association.recording_id=new.recording_id AND association.provider_kind=new.provider_kind
        AND association.plugin_installation_id IS new.plugin_installation_id
)) BEGIN
    SELECT RAISE(ABORT,'Album edition choices require the account''s own artist association');
END;
CREATE TABLE album_edition_choice_receipts (
    user_id TEXT NOT NULL REFERENCES users(id),
    request_id TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    receipt TEXT NOT NULL CHECK(json_valid(receipt) AND length(CAST(receipt AS BLOB))<=4096),
    PRIMARY KEY(user_id,request_id)
);
CREATE TRIGGER album_edition_receipt_immutable BEFORE UPDATE ON album_edition_choice_receipts BEGIN
    SELECT RAISE(ABORT,'Album edition receipts are immutable');
END;
CREATE TRIGGER album_edition_receipt_retained BEFORE DELETE ON album_edition_choice_receipts BEGIN
    SELECT RAISE(ABORT,'Album edition receipts must be retained');
END;
