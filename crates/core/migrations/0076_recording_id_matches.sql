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
