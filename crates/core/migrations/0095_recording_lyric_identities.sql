CREATE TABLE recording_lyric_identities (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    installation_id TEXT NOT NULL REFERENCES plugin_installations(id),
    provider_recording_id TEXT NOT NULL,
    provider_revision TEXT,
    reviewed_metadata TEXT NOT NULL CHECK(length(CAST(reviewed_metadata AS BLOB)) <= 32768 AND json_valid(reviewed_metadata)),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision >= 1),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE UNIQUE INDEX recording_lyric_identity_exact ON recording_lyric_identities(
    user_id,recording_id,installation_id,provider_recording_id,ifnull(provider_revision,''));
CREATE INDEX recording_lyric_identities_recording ON recording_lyric_identities(user_id,recording_id,id);
CREATE INDEX recording_lyric_identities_retention ON recording_lyric_identities(recording_id);
CREATE INDEX recording_lyric_identities_installation ON recording_lyric_identities(installation_id);
CREATE TRIGGER recording_lyric_identity_owner BEFORE INSERT ON recording_lyric_identities
WHEN NOT EXISTS(SELECT 1 FROM plugin_installations installation
    JOIN recordings recording ON recording.id=new.recording_id
    JOIN library_access_grants access ON access.library_id=recording.library_id AND access.user_id=new.user_id
    WHERE installation.id=new.installation_id AND installation.user_id=new.user_id) BEGIN
    SELECT RAISE(ABORT, 'Lyric identities require an owned installation and recording access');
END;
CREATE TRIGGER recording_lyric_identity_limit BEFORE INSERT ON recording_lyric_identities
WHEN (SELECT count(*) FROM recording_lyric_identities WHERE user_id=new.user_id AND recording_id=new.recording_id)>=8 BEGIN
    SELECT RAISE(ABORT, 'A recording can retain at most eight independent lyric identities per account');
END;
CREATE TRIGGER recording_lyric_identity_immutable BEFORE UPDATE ON recording_lyric_identities BEGIN
    SELECT RAISE(ABORT, 'Reviewed lyric identities are immutable');
END;
