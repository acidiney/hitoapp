CREATE TABLE bootstrap_metadata (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    version INTEGER NOT NULL CHECK(version = 4),
    state_id BLOB NOT NULL CHECK(length(state_id) = 16),
    provider_authority BLOB NOT NULL CHECK(length(provider_authority) = 32),
    catalog_witness BLOB NOT NULL CHECK(length(catalog_witness) = 32),
    receipt_count BLOB NOT NULL CHECK(length(receipt_count) = 8 AND receipt_count BETWEEN X'0000000000000000' AND X'0000000000000400'),
    receipt_digest BLOB NOT NULL CHECK(length(receipt_digest) = 32),
    mac BLOB NOT NULL CHECK(length(mac) = 32)
) STRICT;

CREATE TABLE operations (
    operation_id BLOB PRIMARY KEY CHECK(length(operation_id) = 16),
    handle BLOB NOT NULL UNIQUE CHECK(length(handle) = 16),
    recording_id TEXT NOT NULL CHECK(length(CAST(recording_id AS BLOB)) BETWEEN 1 AND 256),
    source_id TEXT NOT NULL CHECK(length(CAST(source_id AS BLOB)) BETWEEN 1 AND 256),
    binding BLOB NOT NULL CHECK(length(binding) BETWEEN 1 AND 16384),
    authority BLOB NOT NULL CHECK(length(authority) = 32),
    phase BLOB NOT NULL CHECK(length(phase) = 8 AND phase BETWEEN X'0000000000000000' AND X'0000000000000006'),
    reason BLOB NOT NULL CHECK(length(reason) = 8 AND reason BETWEEN X'0000000000000000' AND X'0000000000000009'),
    retry_at_ms BLOB NOT NULL CHECK(length(retry_at_ms) = 8 AND retry_at_ms BETWEEN X'0000000000000000' AND X'7fffffffffffffff'),
    revision BLOB NOT NULL CHECK(length(revision) = 8 AND revision BETWEEN X'0000000000000000' AND X'7fffffffffffffff'),
    current BLOB NOT NULL CHECK(length(current) = 8 AND current IN (X'0000000000000000', X'0000000000000001')),
    candidate BLOB NOT NULL CHECK(length(candidate) IN (0, 16384)),
    candidate_length BLOB NOT NULL CHECK(length(candidate_length) = 8 AND candidate_length BETWEEN X'0000000000000000' AND X'0000000000004000'),
    mac BLOB NOT NULL CHECK(length(mac) = 32),
    CHECK((phase = X'0000000000000006' AND length(candidate) = 0 AND candidate_length = X'0000000000000000') OR (phase <> X'0000000000000006' AND length(candidate) = 16384)),
    CHECK((phase = X'0000000000000004' AND candidate_length >= X'000000000000001e') OR (phase <> X'0000000000000004' AND candidate_length = X'0000000000000000')),
    CHECK((phase IN (X'0000000000000002', X'0000000000000003') AND retry_at_ms > X'0000000000000000') OR (phase NOT IN (X'0000000000000002', X'0000000000000003') AND retry_at_ms = X'0000000000000000')),
    CHECK((phase IN (X'0000000000000003', X'0000000000000006') AND reason > X'0000000000000000') OR (phase NOT IN (X'0000000000000003', X'0000000000000006') AND reason = X'0000000000000000')),
    CHECK(current = X'0000000000000001' OR phase = X'0000000000000006')
) STRICT;

CREATE UNIQUE INDEX current_source_operation ON operations(recording_id, source_id) WHERE current = X'0000000000000001';

CREATE INDEX operation_receipt_macs ON operations(operation_id, mac);
