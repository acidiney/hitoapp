CREATE TABLE recording_artist_association_changes (
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    revision INTEGER NOT NULL CHECK(revision>0),
    association_id TEXT NOT NULL REFERENCES recording_artist_associations(id),
    linked INTEGER NOT NULL CHECK(linked IN (0,1)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(user_id,recording_id,revision)
);
CREATE INDEX recording_artist_association_change_association
    ON recording_artist_association_changes(user_id,association_id,revision);
CREATE TRIGGER recording_artist_association_change_scope BEFORE INSERT ON recording_artist_association_changes
WHEN NOT EXISTS (
    SELECT 1 FROM recording_artist_associations association
    WHERE association.id=new.association_id AND association.user_id=new.user_id
        AND association.recording_id=new.recording_id
) BEGIN
    SELECT RAISE(ABORT,'Artist association changes require the account''s own association');
END;
CREATE TRIGGER recording_artist_association_change_immutable BEFORE UPDATE ON recording_artist_association_changes BEGIN
    SELECT RAISE(ABORT,'Artist association changes are immutable');
END;
CREATE TRIGGER recording_artist_association_change_retained BEFORE DELETE ON recording_artist_association_changes BEGIN
    SELECT RAISE(ABORT,'Artist association changes must be retained');
END;
CREATE TABLE recording_artist_association_receipts (
    user_id TEXT NOT NULL REFERENCES users(id),
    request_id TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    receipt TEXT NOT NULL CHECK(json_valid(receipt) AND length(CAST(receipt AS BLOB))<=4096),
    PRIMARY KEY(user_id,request_id)
);
CREATE TRIGGER recording_artist_association_receipt_immutable BEFORE UPDATE ON recording_artist_association_receipts BEGIN
    SELECT RAISE(ABORT,'Artist association receipts are immutable');
END;
CREATE TRIGGER recording_artist_association_receipt_retained BEFORE DELETE ON recording_artist_association_receipts BEGIN
    SELECT RAISE(ABORT,'Artist association receipts must be retained');
END;
