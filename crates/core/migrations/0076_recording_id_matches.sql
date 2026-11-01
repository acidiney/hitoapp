-- A recording whose own file tags name a MusicBrainz recording with exactly one credited artist
-- is matched to that artist. The provenance table keeps that the match came from the tag.
CREATE TABLE artist_association_recording_id_matches (
    association_id TEXT PRIMARY KEY REFERENCES recording_artist_associations(id),
    receipt_order INTEGER NOT NULL REFERENCES artist_identity_choice_receipts(acceptance_order),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TRIGGER artist_association_recording_id_match_immutable BEFORE UPDATE ON artist_association_recording_id_matches BEGIN
    SELECT RAISE(ABORT,'Artist association recording identifier match provenance is immutable');
END;
CREATE TRIGGER artist_association_recording_id_match_retained BEFORE DELETE ON artist_association_recording_id_matches
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=(SELECT a.user_id FROM recording_artist_associations a WHERE a.id=old.association_id))
    OR EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='recording' AND s.subject=(SELECT a.recording_id FROM recording_artist_associations a WHERE a.id=old.association_id))) BEGIN
    SELECT RAISE(ABORT,'Artist association recording identifier match provenance must be retained');
END;

-- Recordings whose tags carry a recording identifier are examined again, and their accounts walk
-- again, even when an earlier pass settled them without it.
INSERT INTO artist_matching_jobs(user_id)
    SELECT DISTINCT m.user_id FROM recording_musicbrainz_observations t
    JOIN recordings r ON r.id=t.recording_id AND r.metadata_observation_id=t.observation_id
    JOIN library_members m ON m.library_id=r.library_id
    WHERE t.entity='recording'
    ON CONFLICT(user_id) DO UPDATE SET rerun=1;
DELETE FROM artist_provider_checks WHERE EXISTS (
    SELECT 1 FROM recording_musicbrainz_observations t
    JOIN recordings r ON r.id=t.recording_id AND r.metadata_observation_id=t.observation_id
    WHERE t.entity='recording' AND r.id=artist_provider_checks.recording_id);
