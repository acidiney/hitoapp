CREATE TABLE source_choice_reset_receipts (
    user_id TEXT NOT NULL REFERENCES users(id),
    request_id TEXT NOT NULL,
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    expected_revision INTEGER NOT NULL CHECK(expected_revision >= 0),
    revision INTEGER NOT NULL CHECK(revision > 0 AND revision = expected_revision + 1),
    PRIMARY KEY(user_id,request_id)
);
CREATE TRIGGER source_choice_reset_receipt_immutable BEFORE UPDATE ON source_choice_reset_receipts BEGIN
    SELECT RAISE(ABORT, 'Source choice reset receipts are immutable');
END;
CREATE TRIGGER source_choice_reset_receipt_retained BEFORE DELETE ON source_choice_reset_receipts BEGIN
    SELECT RAISE(ABORT, 'Source choice reset receipts must be retained');
END;
