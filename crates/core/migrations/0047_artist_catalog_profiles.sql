CREATE TABLE account_metadata_policies (
    user_id TEXT PRIMARY KEY REFERENCES users(id),
    revision INTEGER NOT NULL CHECK(revision > 0),
    enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
    providers TEXT NOT NULL CHECK(json_valid(providers) AND length(CAST(providers AS BLOB)) <= 4096)
);

CREATE TABLE recording_artist_associations (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    library_id TEXT NOT NULL REFERENCES libraries(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    artist_id TEXT NOT NULL,
    source_id TEXT REFERENCES sources(id),
    observation_id TEXT NOT NULL REFERENCES metadata_observations(id),
    role TEXT NOT NULL CHECK(role = 'recording_artist'),
    provider_kind TEXT NOT NULL CHECK(provider_kind IN ('builtin_musicbrainz','http_plugin')),
    plugin_installation_id TEXT REFERENCES plugin_installations(id),
    provider_key TEXT GENERATED ALWAYS AS (
        CASE WHEN provider_kind = 'builtin_musicbrainz' THEN 'builtin:musicbrainz'
             ELSE 'http:' || plugin_installation_id END
    ) STORED,
    provider_artist_id TEXT NOT NULL CHECK(length(CAST(provider_artist_id AS BLOB)) BETWEEN 1 AND 256),
    evidence TEXT NOT NULL CHECK(evidence = 'user_confirmed'),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision = 1),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    FOREIGN KEY(user_id,artist_id) REFERENCES account_artists(user_id,id),
    CHECK((provider_kind = 'builtin_musicbrainz' AND plugin_installation_id IS NULL)
        OR (provider_kind = 'http_plugin' AND plugin_installation_id IS NOT NULL)),
    UNIQUE(user_id,recording_id,artist_id,observation_id,role,provider_key,provider_artist_id)
);
CREATE INDEX recording_artist_associations_account ON recording_artist_associations(user_id,recording_id,created_at,id);
CREATE TRIGGER recording_artist_association_scope BEFORE INSERT ON recording_artist_associations
WHEN NOT EXISTS (
    SELECT 1 FROM recordings r
    JOIN libraries library ON library.id = r.library_id
    JOIN library_members access ON access.library_id = r.library_id AND access.user_id = new.user_id
    JOIN account_artists artist ON artist.user_id = access.user_id AND artist.id = new.artist_id AND artist.library_id = r.library_id
    JOIN account_recording_credits credit ON credit.user_id = access.user_id AND credit.recording_id = r.id AND credit.artist_id = artist.id
    JOIN metadata_observations observation ON observation.id = new.observation_id
        AND observation.id = r.metadata_observation_id AND observation.recording_id = r.id
        AND observation.source_id IS new.source_id
    LEFT JOIN sources source ON source.id = observation.source_id AND source.recording_id = r.id
    LEFT JOIN source_metadata source_state ON source_state.source_id = source.id AND source_state.observation_id = observation.id
    LEFT JOIN provider_items primary_binding ON primary_binding.id = r.metadata_binding_id
        AND primary_binding.id = observation.provider_binding_id AND primary_binding.recording_id = r.id
        AND primary_binding.installation_id = observation.plugin_installation_id
        AND primary_binding.provider_recording_id = observation.provider_recording_id
        AND primary_binding.role = 'primary' AND primary_binding.retired = 0
    LEFT JOIN plugin_installations origin ON origin.id = primary_binding.installation_id AND origin.user_id = access.user_id
    WHERE r.id = new.recording_id AND r.library_id = new.library_id
        AND ((observation.source_id IS NOT NULL AND source.id IS NOT NULL AND source_state.observation_id = observation.id)
            OR (observation.source_id IS NULL AND observation.origin = 'provider_response'
                AND primary_binding.id IS NOT NULL AND origin.id IS NOT NULL AND library.owner_id = access.user_id))
) OR (new.plugin_installation_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM plugin_installations installation
    WHERE installation.id = new.plugin_installation_id AND installation.user_id = new.user_id
)) BEGIN
    SELECT RAISE(ABORT, 'Artist associations require their account credit and current observation');
END;
CREATE TRIGGER recording_artist_association_immutable BEFORE UPDATE ON recording_artist_associations BEGIN
    SELECT RAISE(ABORT, 'Artist association evidence is immutable');
END;
CREATE TRIGGER recording_artist_association_retained BEFORE DELETE ON recording_artist_associations BEGIN
    SELECT RAISE(ABORT, 'Artist association evidence must be retained');
END;
