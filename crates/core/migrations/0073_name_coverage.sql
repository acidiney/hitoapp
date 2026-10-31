-- A credit whose one exact-name artist is corroborated by most of the tracks that were asked
-- about gives the rest of its undecided tracks to that artist. The provenance keeps how many
-- tracks supplied the decision.
CREATE TABLE artist_association_coverage_matches (
    association_id TEXT PRIMARY KEY REFERENCES recording_artist_associations(id),
    matched_tracks INTEGER NOT NULL CHECK(matched_tracks>=1),
    probed_tracks INTEGER NOT NULL CHECK(probed_tracks>=matched_tracks),
    receipt_order INTEGER NOT NULL REFERENCES artist_identity_choice_receipts(acceptance_order),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TRIGGER artist_association_coverage_match_immutable BEFORE UPDATE ON artist_association_coverage_matches BEGIN
    SELECT RAISE(ABORT,'Artist association coverage match provenance is immutable');
END;
CREATE TRIGGER artist_association_coverage_match_retained BEFORE DELETE ON artist_association_coverage_matches
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=(SELECT a.user_id FROM recording_artist_associations a WHERE a.id=old.association_id))
    OR EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='recording' AND s.subject=(SELECT a.recording_id FROM recording_artist_associations a WHERE a.id=old.association_id))) BEGIN
    SELECT RAISE(ABORT,'Artist association coverage match provenance must be retained');
END;

-- Name checks of the earlier rule no longer hide a recording (rule 3 compares version titles and
-- continues an artist already matched for the credit), so every account that had any walks again.
INSERT INTO artist_matching_jobs(user_id)
    SELECT DISTINCT user_id FROM artist_name_checks WHERE true
    ON CONFLICT(user_id) DO UPDATE SET rerun=1;
