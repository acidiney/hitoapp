CREATE TABLE account_recording_credits (
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    artist_id TEXT NOT NULL,
    PRIMARY KEY(user_id, recording_id),
    FOREIGN KEY(user_id, artist_id) REFERENCES account_artists(user_id, id)
);
CREATE INDEX account_recording_artists ON account_recording_credits(user_id, artist_id, recording_id);
CREATE INDEX account_recording_lookup ON account_recording_credits(recording_id, user_id);

WITH targets AS (
    SELECT DISTINCT access.user_id, r.library_id, COALESCE(p.override_artist, r.artist) AS name
    FROM recordings r JOIN library_members access ON access.library_id = r.library_id
    LEFT JOIN personal_recording_metadata p ON p.user_id = access.user_id AND p.recording_id = r.id
)
INSERT INTO account_artists(user_id, id, library_id, name)
SELECT t.user_id, COALESCE(shared.id, lower(hex(randomblob(16)))), t.library_id, t.name FROM targets t
LEFT JOIN artists shared ON shared.library_id = t.library_id AND shared.name = t.name COLLATE BINARY WHERE 1
ON CONFLICT(user_id, library_id, name) DO NOTHING;

INSERT INTO account_recording_credits(user_id, recording_id, artist_id)
SELECT access.user_id, r.id, c.id
FROM recordings r CROSS JOIN library_members access ON access.library_id = r.library_id
LEFT JOIN personal_recording_metadata p ON p.user_id = access.user_id AND p.recording_id = r.id
CROSS JOIN account_artists c ON c.user_id = access.user_id AND c.library_id = r.library_id
    AND c.name = COALESCE(p.override_artist, r.artist) COLLATE BINARY;
