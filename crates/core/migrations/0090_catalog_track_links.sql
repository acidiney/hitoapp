CREATE TABLE catalog_track_links (
    user_id TEXT NOT NULL REFERENCES users(id),
    provider_kind TEXT NOT NULL CHECK(provider_kind IN ('builtin_musicbrainz','http_plugin')),
    plugin_installation_id TEXT REFERENCES plugin_installations(id),
    edition_id TEXT NOT NULL CHECK(length(CAST(edition_id AS BLOB)) BETWEEN 1 AND 256),
    occurrence_id TEXT NOT NULL CHECK(length(CAST(occurrence_id AS BLOB)) BETWEEN 1 AND 256),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK((provider_kind='builtin_musicbrainz' AND plugin_installation_id IS NULL)
        OR (provider_kind='http_plugin' AND plugin_installation_id IS NOT NULL))
);
CREATE UNIQUE INDEX catalog_track_links_by_occurrence
    ON catalog_track_links(user_id, provider_kind, COALESCE(plugin_installation_id, ''), edition_id, occurrence_id);
CREATE INDEX catalog_track_links_by_recording ON catalog_track_links(recording_id);
