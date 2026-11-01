-- An account sees one card for recordings that Hito can prove are the same song. Grouping only
-- changes how lists present them: every recording, source and personal reference stays as it is.
CREATE TABLE recording_group_members (
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    canonical_id TEXT NOT NULL REFERENCES recordings(id),
    proof TEXT NOT NULL CHECK(proof IN ('musicbrainz_recording','isrc')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(user_id,recording_id),
    CHECK(canonical_id <> recording_id)
);
CREATE INDEX recording_group_members_canonical ON recording_group_members(user_id,canonical_id,recording_id);
CREATE INDEX recording_group_members_canonical_only ON recording_group_members(canonical_id);
CREATE TRIGGER recording_group_members_flat BEFORE INSERT ON recording_group_members
WHEN EXISTS (SELECT 1 FROM recording_group_members g WHERE g.user_id=new.user_id AND g.recording_id=new.canonical_id)
    OR EXISTS (SELECT 1 FROM recording_group_members g WHERE g.user_id=new.user_id AND g.canonical_id=new.recording_id) BEGIN
    SELECT RAISE(ABORT,'Recording groups are one level deep');
END;
CREATE TRIGGER recording_group_members_immutable BEFORE UPDATE ON recording_group_members BEGIN
    SELECT RAISE(ABORT,'Recording group members are replaced, not edited');
END;

-- "These are different songs": a pair the account separated is never grouped again.
CREATE TABLE recording_group_splits (
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    other_id TEXT NOT NULL REFERENCES recordings(id),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(user_id,recording_id,other_id),
    CHECK(recording_id < other_id)
);
CREATE INDEX recording_group_splits_other ON recording_group_splits(user_id,other_id,recording_id);

CREATE INDEX recording_musicbrainz_observation_mbid ON recording_musicbrainz_observations(entity,mbid,recording_id);
CREATE INDEX recording_isrc_observation_isrc ON recording_isrc_observations(isrc,recording_id);

INSERT INTO artist_matching_jobs(user_id)
    SELECT DISTINCT m.user_id FROM library_members m
    WHERE EXISTS (SELECT 1 FROM recordings r JOIN recording_isrc_observations i ON i.recording_id=r.id AND i.observation_id=r.metadata_observation_id WHERE r.library_id=m.library_id)
       OR EXISTS (SELECT 1 FROM recordings r JOIN recording_musicbrainz_observations t ON t.recording_id=r.id AND t.observation_id=r.metadata_observation_id AND t.entity='recording' WHERE r.library_id=m.library_id)
    ON CONFLICT(user_id) DO UPDATE SET rerun=1;
