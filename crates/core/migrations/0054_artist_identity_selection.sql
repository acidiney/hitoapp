CREATE TABLE account_artist_identity_groups (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    provider_kind TEXT NOT NULL CHECK(provider_kind IN ('builtin_musicbrainz','http_plugin')),
    plugin_installation_id TEXT REFERENCES plugin_installations(id),
    provider_key TEXT GENERATED ALWAYS AS (
        CASE WHEN provider_kind='builtin_musicbrainz' THEN 'builtin:musicbrainz'
             ELSE 'http:' || plugin_installation_id END
    ) STORED,
    provider_artist_id TEXT NOT NULL CHECK(length(CAST(provider_artist_id AS BLOB)) BETWEEN 1 AND 256),
    CHECK((provider_kind='builtin_musicbrainz' AND plugin_installation_id IS NULL)
        OR (provider_kind='http_plugin' AND plugin_installation_id IS NOT NULL)),
    UNIQUE(user_id,id),
    UNIQUE(user_id,provider_key,provider_artist_id)
);
CREATE TABLE recording_artist_identity_choices (
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    revision INTEGER NOT NULL CHECK(revision>0),
    association_id TEXT REFERENCES recording_artist_associations(id),
    group_id TEXT,
    artist_id TEXT,
    artist_operation_id TEXT,
    observation_id TEXT REFERENCES metadata_observations(id),
    retired INTEGER NOT NULL DEFAULT 0 CHECK(retired IN (0,1)),
    selected_at INTEGER NOT NULL DEFAULT (unixepoch()),
    selection_order INTEGER REFERENCES artist_identity_choice_receipts(acceptance_order),
    PRIMARY KEY(user_id,recording_id),
    FOREIGN KEY(user_id,group_id) REFERENCES account_artist_identity_groups(user_id,id),
    FOREIGN KEY(user_id,artist_id) REFERENCES account_artists(user_id,id),
    CHECK((association_id IS NULL AND group_id IS NULL AND artist_id IS NULL AND observation_id IS NULL AND selection_order IS NULL)
        OR (association_id IS NOT NULL AND group_id IS NOT NULL AND artist_id IS NOT NULL AND observation_id IS NOT NULL AND selection_order IS NOT NULL))
);
CREATE INDEX recording_artist_identity_group ON recording_artist_identity_choices(user_id,group_id,retired,recording_id);
CREATE INDEX recording_artist_identity_recording ON recording_artist_identity_choices(recording_id,user_id)
    WHERE retired=0 AND association_id IS NOT NULL;
CREATE TABLE artist_identity_choice_receipts (
    acceptance_order INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id TEXT NOT NULL REFERENCES users(id),
    request_id TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    receipt TEXT NOT NULL CHECK(json_valid(receipt) AND length(CAST(receipt AS BLOB))<=4096),
    UNIQUE(user_id,request_id)
);
CREATE TRIGGER artist_identity_receipt_immutable BEFORE UPDATE ON artist_identity_choice_receipts BEGIN
    SELECT RAISE(ABORT,'Artist identity receipts are immutable');
END;
CREATE TRIGGER artist_identity_receipt_retained BEFORE DELETE ON artist_identity_choice_receipts BEGIN
    SELECT RAISE(ABORT,'Artist identity receipts must be retained');
END;
