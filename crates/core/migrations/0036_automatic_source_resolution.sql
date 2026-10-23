CREATE INDEX recording_metadata_binding ON recordings(metadata_binding_id);
CREATE INDEX artwork_binding ON artwork_fetches(binding_id);
CREATE INDEX source_preparation_binding ON source_preparations(binding_id);
CREATE INDEX download_intent_binding ON download_intents(binding_id);
CREATE INDEX provider_sources_binding_reference ON provider_sources(binding_id,installation_id,recording_id);

PRAGMA defer_foreign_keys = ON;
CREATE TEMP TABLE hito_resolution_bindings AS SELECT rowid AS preserved_rowid,* FROM provider_items;
DROP TABLE provider_items;
CREATE TABLE provider_items (
    id TEXT PRIMARY KEY,
    installation_id TEXT NOT NULL REFERENCES plugin_installations(id),
    provider_recording_id TEXT NOT NULL,
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    role TEXT NOT NULL CHECK(role IN ('primary','source')),
    evidence TEXT NOT NULL CHECK(evidence IN ('provider_identity','user_confirmed','automatic_policy')),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision > 0),
    retired INTEGER NOT NULL DEFAULT 0 CHECK(retired IN (0,1)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(id,installation_id,recording_id)
);
INSERT INTO provider_items(rowid,id,installation_id,provider_recording_id,recording_id,role,evidence,revision,retired,created_at)
SELECT preserved_rowid,id,installation_id,provider_recording_id,recording_id,role,evidence,revision,retired,created_at
FROM hito_resolution_bindings;
DROP TABLE hito_resolution_bindings;
CREATE INDEX provider_items_recording ON provider_items(recording_id,retired,id);
CREATE UNIQUE INDEX provider_items_current_identity ON provider_items(installation_id,provider_recording_id) WHERE retired = 0;
CREATE TRIGGER provider_item_owner BEFORE INSERT ON provider_items
WHEN NOT EXISTS(SELECT 1 FROM plugin_installations p JOIN libraries l ON l.owner_id = p.user_id
    JOIN recordings r ON r.library_id = l.id WHERE p.id = new.installation_id AND r.id = new.recording_id) BEGIN
    SELECT RAISE(ABORT, 'Provider recordings require their account library');
END;
CREATE TRIGGER provider_item_immutable BEFORE UPDATE ON provider_items
WHEN old.id IS NOT new.id OR old.installation_id IS NOT new.installation_id
    OR old.provider_recording_id IS NOT new.provider_recording_id OR old.recording_id IS NOT new.recording_id
    OR old.role IS NOT new.role OR old.evidence IS NOT new.evidence OR old.created_at IS NOT new.created_at
    OR old.retired != 0 OR new.retired != 1 OR new.revision != old.revision + 1 BEGIN
    SELECT RAISE(ABORT, 'Provider binding identity is immutable');
END;
CREATE TRIGGER provider_item_retained BEFORE DELETE ON provider_items BEGIN
    SELECT RAISE(ABORT, 'Provider binding history must be retained');
END;

CREATE TABLE recording_source_choices (
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    revision INTEGER NOT NULL DEFAULT 0 CHECK(revision >= 0),
    PRIMARY KEY(user_id,recording_id)
);
CREATE TABLE source_resolutions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    target_digest TEXT NOT NULL CHECK(length(target_digest) = 64),
    choice_revision INTEGER NOT NULL CHECK(choice_revision >= 0),
    outcome TEXT CHECK(outcome IN ('ready','no_suitable_source','protected_choice','cancelled')),
    lease_token TEXT,
    lease_until INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    completed_at INTEGER,
    CHECK((lease_token IS NULL) = (lease_until IS NULL)),
    CHECK((outcome IS NULL) = (lease_token IS NOT NULL)),
    CHECK((outcome IS NULL) = (completed_at IS NULL))
);
CREATE UNIQUE INDEX source_resolution_running_account ON source_resolutions(user_id) WHERE outcome IS NULL;
CREATE INDEX source_resolution_running ON source_resolutions(lease_until,id) WHERE outcome IS NULL;
CREATE INDEX source_resolution_recording ON source_resolutions(user_id,recording_id,created_at);
CREATE TABLE source_resolution_receipts (
    user_id TEXT NOT NULL REFERENCES users(id),
    request_id TEXT NOT NULL,
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    resolution_id TEXT NOT NULL REFERENCES source_resolutions(id),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY(user_id,request_id)
);
CREATE INDEX source_resolution_receipt_resolution ON source_resolution_receipts(resolution_id);
CREATE TABLE automatic_source_decisions (
    resolution_id TEXT PRIMARY KEY REFERENCES source_resolutions(id),
    binding_id TEXT NOT NULL REFERENCES provider_items(id),
    policy_version TEXT NOT NULL,
    target_digest TEXT NOT NULL CHECK(length(target_digest) = 64),
    candidate_digest TEXT NOT NULL CHECK(length(candidate_digest) = 64),
    installation_revision INTEGER NOT NULL CHECK(installation_revision > 0),
    service_revision INTEGER NOT NULL CHECK(service_revision > 0),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TRIGGER source_resolution_receipt_immutable BEFORE UPDATE ON source_resolution_receipts BEGIN
    SELECT RAISE(ABORT, 'Source resolution receipts are immutable');
END;
CREATE TRIGGER source_resolution_receipt_retained BEFORE DELETE ON source_resolution_receipts BEGIN
    SELECT RAISE(ABORT, 'Source resolution receipts must be retained');
END;
CREATE TRIGGER automatic_source_decision_immutable BEFORE UPDATE ON automatic_source_decisions BEGIN
    SELECT RAISE(ABORT, 'Automatic source decisions are immutable');
END;
CREATE TRIGGER automatic_source_decision_retained BEFORE DELETE ON automatic_source_decisions BEGIN
    SELECT RAISE(ABORT, 'Automatic source decisions must be retained');
END;
CREATE TRIGGER source_resolution_identity_immutable BEFORE UPDATE OF id,user_id,recording_id,target_digest,choice_revision,created_at ON source_resolutions BEGIN
    SELECT RAISE(ABORT, 'Source resolution identity is immutable');
END;
CREATE TRIGGER source_resolution_terminal_immutable BEFORE UPDATE ON source_resolutions WHEN old.outcome IS NOT NULL BEGIN
    SELECT RAISE(ABORT, 'Source resolution terminal results are immutable');
END;
CREATE TEMP TABLE hito_resolution_fk_guard (value INTEGER NOT NULL CHECK(value = 0));
INSERT INTO hito_resolution_fk_guard SELECT 1 FROM pragma_foreign_key_check;
DROP TABLE hito_resolution_fk_guard;
PRAGMA defer_foreign_keys = OFF;
