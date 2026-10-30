CREATE TABLE correction_erasure_scopes (
    kind TEXT NOT NULL CHECK(kind IN ('account','recording')),
    subject TEXT NOT NULL CHECK(length(subject) BETWEEN 1 AND 64),
    PRIMARY KEY(kind,subject)
);
DROP TRIGGER recording_artist_association_retained;
CREATE TRIGGER recording_artist_association_retained BEFORE DELETE ON recording_artist_associations
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.user_id)
    OR EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='recording' AND s.subject=old.recording_id)) BEGIN
    SELECT RAISE(ABORT,'Artist association evidence must be retained');
END;
DROP TRIGGER recording_artist_association_change_retained;
CREATE TRIGGER recording_artist_association_change_retained BEFORE DELETE ON recording_artist_association_changes
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.user_id)
    OR EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='recording' AND s.subject=old.recording_id)) BEGIN
    SELECT RAISE(ABORT,'Artist association changes must be retained');
END;
DROP TRIGGER recording_artist_association_receipt_retained;
CREATE TRIGGER recording_artist_association_receipt_retained BEFORE DELETE ON recording_artist_association_receipts
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.user_id)) BEGIN
    SELECT RAISE(ABORT,'Artist association receipts must be retained');
END;
DROP TRIGGER recording_artist_association_change_order_retained;
CREATE TRIGGER recording_artist_association_change_order_retained BEFORE DELETE ON recording_artist_association_change_order
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.user_id)
    OR EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='recording' AND s.subject=old.recording_id)) BEGIN
    SELECT RAISE(ABORT,'Artist association change order must be retained');
END;
DROP TRIGGER artist_association_change_retirement_retained;
CREATE TRIGGER artist_association_change_retirement_retained BEFORE DELETE ON artist_association_change_retirements
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.user_id)
    OR EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='recording' AND s.subject=(SELECT a.recording_id FROM recording_artist_associations a WHERE a.id=old.association_id))) BEGIN
    SELECT RAISE(ABORT,'Artist association retirements must be retained');
END;
DROP TRIGGER artist_identity_change_retained;
CREATE TRIGGER artist_identity_change_retained BEFORE DELETE ON artist_identity_changes
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.user_id)) BEGIN
    SELECT RAISE(ABORT,'Artist identity changes must be retained');
END;
DROP TRIGGER artist_identity_change_item_retained;
CREATE TRIGGER artist_identity_change_item_retained BEFORE DELETE ON artist_identity_change_items
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=(SELECT c.user_id FROM artist_identity_changes c WHERE c.id=old.change_id))
    OR EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='recording' AND s.subject=old.recording_id)) BEGIN
    SELECT RAISE(ABORT,'Artist identity change items must be retained');
END;
DROP TRIGGER artist_identity_receipt_retained;
CREATE TRIGGER artist_identity_receipt_retained BEFORE DELETE ON artist_identity_choice_receipts
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.user_id)) BEGIN
    SELECT RAISE(ABORT,'Artist identity receipts must be retained');
END;
DROP TRIGGER album_edition_receipt_retained;
CREATE TRIGGER album_edition_receipt_retained BEFORE DELETE ON album_edition_choice_receipts
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.user_id)) BEGIN
    SELECT RAISE(ABORT,'Album edition receipts must be retained');
END;
DROP TRIGGER shared_artist_correction_receipt_retained;
CREATE TRIGGER shared_artist_correction_receipt_retained BEFORE DELETE ON shared_artist_correction_receipts
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.user_id)) BEGIN
    SELECT RAISE(ABORT,'Shared artist correction receipts must be retained');
END;
DROP TRIGGER artist_association_match_retained;
CREATE TRIGGER artist_association_match_retained BEFORE DELETE ON artist_association_matches
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=(SELECT a.user_id FROM recording_artist_associations a WHERE a.id=old.association_id))
    OR EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='recording' AND s.subject=(SELECT a.recording_id FROM recording_artist_associations a WHERE a.id=old.association_id))) BEGIN
    SELECT RAISE(ABORT,'Artist association match provenance must be retained');
END;
DROP TRIGGER artist_association_provider_match_retained;
CREATE TRIGGER artist_association_provider_match_retained BEFORE DELETE ON artist_association_provider_matches
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=(SELECT a.user_id FROM recording_artist_associations a WHERE a.id=old.association_id))
    OR EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='recording' AND s.subject=(SELECT a.recording_id FROM recording_artist_associations a WHERE a.id=old.association_id))) BEGIN
    SELECT RAISE(ABORT,'Artist association provider match provenance must be retained');
END;
DROP TRIGGER artist_association_name_match_retained;
CREATE TRIGGER artist_association_name_match_retained BEFORE DELETE ON artist_association_name_matches
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=(SELECT a.user_id FROM recording_artist_associations a WHERE a.id=old.association_id))
    OR EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='recording' AND s.subject=(SELECT a.recording_id FROM recording_artist_associations a WHERE a.id=old.association_id))) BEGIN
    SELECT RAISE(ABORT,'Artist association name match provenance must be retained');
END;
DROP TRIGGER shared_artist_correction_subject_retained;
CREATE TRIGGER shared_artist_correction_subject_retained BEFORE DELETE ON shared_artist_correction_subjects
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='recording' AND s.subject=old.recording_id)) BEGIN
    SELECT RAISE(ABORT,'Shared artist correction subjects must be retained');
END;
DROP TRIGGER recording_isrc_observation_retained;
CREATE TRIGGER recording_isrc_observation_retained BEFORE DELETE ON recording_isrc_observations
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='recording' AND s.subject=old.recording_id)) BEGIN
    SELECT RAISE(ABORT,'ISRC observations must be retained');
END;
DROP TRIGGER recording_artist_url_observation_retained;
CREATE TRIGGER recording_artist_url_observation_retained BEFORE DELETE ON recording_artist_url_observations
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='recording' AND s.subject=old.recording_id)) BEGIN
    SELECT RAISE(ABORT,'Artist page observations must be retained');
END;
DROP TRIGGER recording_musicbrainz_observation_retained;
CREATE TRIGGER recording_musicbrainz_observation_retained BEFORE DELETE ON recording_musicbrainz_observations
WHEN NOT (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='recording' AND s.subject=old.recording_id)) BEGIN
    SELECT RAISE(ABORT,'MusicBrainz tag identity observations must be retained');
END;

PRAGMA defer_foreign_keys = ON;
CREATE TABLE shared_artist_corrections_next (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    promoted_by TEXT REFERENCES users(id),
    source_kind TEXT NOT NULL CHECK(source_kind IN ('unlink','split')),
    source_change_id TEXT REFERENCES artist_identity_changes(id),
    source_association_id TEXT REFERENCES recording_artist_associations(id),
    change_floor INTEGER NOT NULL CHECK(change_floor>=0),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK((source_kind='unlink' AND source_change_id IS NULL)
        OR (source_kind='split' AND source_association_id IS NULL))
);
INSERT INTO shared_artist_corrections_next(seq,id,promoted_by,source_kind,source_change_id,source_association_id,change_floor,created_at)
    SELECT seq,id,promoted_by,source_kind,source_change_id,source_association_id,change_floor,created_at FROM shared_artist_corrections ORDER BY seq;
CREATE TABLE shared_artist_correction_states_next (
    correction_seq INTEGER NOT NULL REFERENCES shared_artist_corrections(seq),
    revision INTEGER NOT NULL CHECK(revision>0),
    applied INTEGER NOT NULL CHECK(applied IN (0,1)),
    changed_by TEXT REFERENCES users(id),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(correction_seq,revision)
);
INSERT INTO shared_artist_correction_states_next(correction_seq,revision,applied,changed_by,created_at)
    SELECT correction_seq,revision,applied,changed_by,created_at FROM shared_artist_correction_states ORDER BY correction_seq,revision;
DROP TABLE shared_artist_correction_states;
DROP TABLE shared_artist_corrections;
ALTER TABLE shared_artist_corrections_next RENAME TO shared_artist_corrections;
ALTER TABLE shared_artist_correction_states_next RENAME TO shared_artist_correction_states;
DELETE FROM sqlite_sequence WHERE name='shared_artist_corrections' AND NOT EXISTS (SELECT 1 FROM shared_artist_corrections);
PRAGMA defer_foreign_keys = OFF;

CREATE TRIGGER shared_artist_correction_immutable BEFORE UPDATE ON shared_artist_corrections
WHEN NOT (
    new.seq IS old.seq AND new.id IS old.id AND new.source_kind IS old.source_kind
    AND new.change_floor IS old.change_floor AND new.created_at IS old.created_at
    AND (new.promoted_by IS old.promoted_by OR (new.promoted_by IS NULL AND EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.promoted_by)))
    AND (new.source_change_id IS old.source_change_id OR (new.source_change_id IS NULL AND EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=(SELECT c.user_id FROM artist_identity_changes c WHERE c.id=old.source_change_id))))
    AND (new.source_association_id IS old.source_association_id
        OR (new.source_association_id IS NULL AND (EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=(SELECT a.user_id FROM recording_artist_associations a WHERE a.id=old.source_association_id)) OR EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='recording' AND s.subject=(SELECT a.recording_id FROM recording_artist_associations a WHERE a.id=old.source_association_id)))))
) BEGIN
    SELECT RAISE(ABORT,'Shared artist corrections are immutable');
END;
CREATE TRIGGER shared_artist_correction_retained BEFORE DELETE ON shared_artist_corrections BEGIN
    SELECT RAISE(ABORT,'Shared artist corrections must be retained');
END;
CREATE TRIGGER shared_artist_correction_administrator BEFORE INSERT ON shared_artist_corrections
WHEN NOT EXISTS (SELECT 1 FROM users WHERE id=new.promoted_by AND admin=1) BEGIN
    SELECT RAISE(ABORT,'Only administrators can share artist corrections');
END;
CREATE TRIGGER shared_artist_correction_source BEFORE INSERT ON shared_artist_corrections
WHEN (new.source_kind='unlink' AND new.source_association_id IS NULL)
    OR (new.source_kind='split' AND new.source_change_id IS NULL) BEGIN
    SELECT RAISE(ABORT,'Shared artist corrections need the correction they were made from');
END;
CREATE TRIGGER shared_artist_correction_state_immutable BEFORE UPDATE ON shared_artist_correction_states
WHEN NOT (
    new.correction_seq IS old.correction_seq AND new.revision IS old.revision
    AND new.applied IS old.applied AND new.created_at IS old.created_at
    AND new.changed_by IS NULL AND old.changed_by IS NOT NULL AND EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.changed_by)
) BEGIN
    SELECT RAISE(ABORT,'Shared artist correction states are immutable');
END;
CREATE TRIGGER shared_artist_correction_state_retained BEFORE DELETE ON shared_artist_correction_states BEGIN
    SELECT RAISE(ABORT,'Shared artist correction states must be retained');
END;
CREATE TRIGGER shared_artist_correction_state_administrator BEFORE INSERT ON shared_artist_correction_states
WHEN NOT EXISTS (SELECT 1 FROM users WHERE id=new.changed_by AND admin=1) BEGIN
    SELECT RAISE(ABORT,'Only administrators can change shared artist corrections');
END;
