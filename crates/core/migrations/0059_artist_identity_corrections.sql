CREATE TABLE artist_identity_changes (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    group_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision>0),
    kind TEXT NOT NULL CHECK(kind IN ('split','undo')),
    destination_group_id TEXT,
    receipt_order INTEGER NOT NULL REFERENCES artist_identity_choice_receipts(acceptance_order),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(user_id,group_id,revision),
    FOREIGN KEY(user_id,group_id) REFERENCES account_artist_identity_groups(user_id,id),
    FOREIGN KEY(user_id,destination_group_id) REFERENCES account_artist_identity_groups(user_id,id)
);
CREATE TRIGGER artist_identity_change_immutable BEFORE UPDATE ON artist_identity_changes BEGIN
    SELECT RAISE(ABORT,'Artist identity changes are immutable');
END;
CREATE TRIGGER artist_identity_change_retained BEFORE DELETE ON artist_identity_changes BEGIN
    SELECT RAISE(ABORT,'Artist identity changes must be retained');
END;
CREATE TABLE artist_identity_change_items (
    change_id TEXT NOT NULL REFERENCES artist_identity_changes(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    from_revision INTEGER NOT NULL CHECK(from_revision>0),
    from_group_id TEXT,
    from_association_id TEXT REFERENCES recording_artist_associations(id),
    from_artist_id TEXT NOT NULL,
    from_operation_id TEXT,
    from_observation_id TEXT,
    from_order INTEGER,
    to_revision INTEGER NOT NULL CHECK(to_revision>from_revision),
    to_group_id TEXT,
    to_association_id TEXT REFERENCES recording_artist_associations(id),
    to_operation_id TEXT,
    to_observation_id TEXT,
    to_order INTEGER,
    retired_reason TEXT CHECK(retired_reason IN ('credit_changed')),
    PRIMARY KEY(change_id,recording_id)
) WITHOUT ROWID;
CREATE INDEX artist_identity_change_item_recording ON artist_identity_change_items(recording_id,retired_reason);
CREATE TRIGGER artist_identity_change_item_immutable BEFORE UPDATE ON artist_identity_change_items
WHEN old.retired_reason IS NOT NULL OR new.retired_reason IS NULL
    OR old.change_id IS NOT new.change_id OR old.recording_id IS NOT new.recording_id
    OR old.from_revision IS NOT new.from_revision OR old.to_revision IS NOT new.to_revision
    OR old.from_group_id IS NOT new.from_group_id OR old.to_group_id IS NOT new.to_group_id
    OR old.from_association_id IS NOT new.from_association_id OR old.to_association_id IS NOT new.to_association_id
    OR old.from_artist_id IS NOT new.from_artist_id OR old.from_operation_id IS NOT new.from_operation_id
    OR old.to_operation_id IS NOT new.to_operation_id OR old.from_observation_id IS NOT new.from_observation_id
    OR old.to_observation_id IS NOT new.to_observation_id OR old.from_order IS NOT new.from_order
    OR old.to_order IS NOT new.to_order
BEGIN
    SELECT RAISE(ABORT,'Artist identity change items only gain a retirement reason');
END;
CREATE TRIGGER artist_identity_change_item_retained BEFORE DELETE ON artist_identity_change_items BEGIN
    SELECT RAISE(ABORT,'Artist identity change items must be retained');
END;
CREATE TABLE artist_association_change_retirements (
    user_id TEXT NOT NULL REFERENCES users(id),
    association_id TEXT NOT NULL REFERENCES recording_artist_associations(id),
    reason TEXT NOT NULL CHECK(reason IN ('credit_changed')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(user_id,association_id)
) WITHOUT ROWID;
CREATE TRIGGER artist_association_change_retirement_immutable BEFORE UPDATE ON artist_association_change_retirements BEGIN
    SELECT RAISE(ABORT,'Artist association retirements are immutable');
END;
CREATE TRIGGER artist_association_change_retirement_retained BEFORE DELETE ON artist_association_change_retirements BEGIN
    SELECT RAISE(ABORT,'Artist association retirements must be retained');
END;
CREATE TABLE shared_artist_corrections (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    promoted_by TEXT NOT NULL REFERENCES users(id),
    source_kind TEXT NOT NULL CHECK(source_kind IN ('unlink','split')),
    source_change_id TEXT REFERENCES artist_identity_changes(id),
    source_association_id TEXT REFERENCES recording_artist_associations(id),
    change_floor INTEGER NOT NULL CHECK(change_floor>=0),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK((source_kind='unlink' AND source_association_id IS NOT NULL AND source_change_id IS NULL)
        OR (source_kind='split' AND source_change_id IS NOT NULL AND source_association_id IS NULL))
);
CREATE TRIGGER shared_artist_correction_immutable BEFORE UPDATE ON shared_artist_corrections BEGIN
    SELECT RAISE(ABORT,'Shared artist corrections are immutable');
END;
CREATE TRIGGER shared_artist_correction_retained BEFORE DELETE ON shared_artist_corrections BEGIN
    SELECT RAISE(ABORT,'Shared artist corrections must be retained');
END;
CREATE TRIGGER shared_artist_correction_administrator BEFORE INSERT ON shared_artist_corrections
WHEN NOT EXISTS (SELECT 1 FROM users WHERE id=new.promoted_by AND admin=1) BEGIN
    SELECT RAISE(ABORT,'Only administrators can share artist corrections');
END;
CREATE TABLE shared_artist_correction_subjects (
    correction_seq INTEGER NOT NULL REFERENCES shared_artist_corrections(seq),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    observation_id TEXT NOT NULL REFERENCES metadata_observations(id),
    provider_artist_id TEXT NOT NULL CHECK(length(CAST(provider_artist_id AS BLOB)) BETWEEN 1 AND 256),
    raw_artist TEXT NOT NULL,
    retired_reason TEXT CHECK(retired_reason IN ('tags_changed')),
    PRIMARY KEY(recording_id,observation_id,provider_artist_id,correction_seq)
) WITHOUT ROWID;
CREATE INDEX shared_artist_correction_subject_correction ON shared_artist_correction_subjects(correction_seq,recording_id);
CREATE TRIGGER shared_artist_correction_subject_immutable BEFORE UPDATE ON shared_artist_correction_subjects
WHEN old.retired_reason IS NOT NULL OR new.retired_reason IS NULL
    OR old.correction_seq IS NOT new.correction_seq OR old.recording_id IS NOT new.recording_id
    OR old.observation_id IS NOT new.observation_id OR old.provider_artist_id IS NOT new.provider_artist_id
    OR old.raw_artist IS NOT new.raw_artist
BEGIN
    SELECT RAISE(ABORT,'Shared artist correction subjects only gain a retirement reason');
END;
CREATE TRIGGER shared_artist_correction_subject_retained BEFORE DELETE ON shared_artist_correction_subjects BEGIN
    SELECT RAISE(ABORT,'Shared artist correction subjects must be retained');
END;
CREATE TABLE shared_artist_correction_states (
    correction_seq INTEGER NOT NULL REFERENCES shared_artist_corrections(seq),
    revision INTEGER NOT NULL CHECK(revision>0),
    applied INTEGER NOT NULL CHECK(applied IN (0,1)),
    changed_by TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(correction_seq,revision)
) WITHOUT ROWID;
CREATE TRIGGER shared_artist_correction_state_immutable BEFORE UPDATE ON shared_artist_correction_states BEGIN
    SELECT RAISE(ABORT,'Shared artist correction states are immutable');
END;
CREATE TRIGGER shared_artist_correction_state_retained BEFORE DELETE ON shared_artist_correction_states BEGIN
    SELECT RAISE(ABORT,'Shared artist correction states must be retained');
END;
CREATE TRIGGER shared_artist_correction_state_administrator BEFORE INSERT ON shared_artist_correction_states
WHEN NOT EXISTS (SELECT 1 FROM users WHERE id=new.changed_by AND admin=1) BEGIN
    SELECT RAISE(ABORT,'Only administrators can change shared artist corrections');
END;
CREATE TABLE shared_artist_correction_receipts (
    user_id TEXT NOT NULL REFERENCES users(id),
    request_id TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    receipt TEXT NOT NULL CHECK(json_valid(receipt) AND length(CAST(receipt AS BLOB))<=4096),
    PRIMARY KEY(user_id,request_id)
) WITHOUT ROWID;
CREATE TRIGGER shared_artist_correction_receipt_immutable BEFORE UPDATE ON shared_artist_correction_receipts BEGIN
    SELECT RAISE(ABORT,'Shared artist correction receipts are immutable');
END;
CREATE TRIGGER shared_artist_correction_receipt_retained BEFORE DELETE ON shared_artist_correction_receipts BEGIN
    SELECT RAISE(ABORT,'Shared artist correction receipts must be retained');
END;
