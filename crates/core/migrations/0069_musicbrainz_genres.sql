CREATE TABLE musicbrainz_genre_lookups (
    entity TEXT NOT NULL CHECK(entity IN ('artist','release_group','recording')),
    mbid TEXT NOT NULL CHECK(length(mbid)=36),
    fetched_at INTEGER NOT NULL,
    PRIMARY KEY(entity, mbid)
);
CREATE TABLE musicbrainz_genres (
    entity TEXT NOT NULL,
    mbid TEXT NOT NULL,
    genre_key TEXT NOT NULL CHECK(length(genre_key) BETWEEN 1 AND 64),
    label TEXT NOT NULL CHECK(length(label) BETWEEN 1 AND 64),
    provenance TEXT NOT NULL DEFAULT 'musicbrainz' CHECK(provenance='musicbrainz'),
    PRIMARY KEY(entity, mbid, genre_key),
    FOREIGN KEY(entity, mbid) REFERENCES musicbrainz_genre_lookups(entity, mbid) ON DELETE CASCADE
);
CREATE INDEX musicbrainz_genres_by_genre ON musicbrainz_genres(genre_key, entity, mbid, label);
CREATE INDEX recording_musicbrainz_observation_by_mbid
    ON recording_musicbrainz_observations(entity, mbid, recording_id, observation_id);
