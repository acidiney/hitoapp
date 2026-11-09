-- The artists named inside a collaboration credit, one row per artist and recording, so each
-- member's page lists the recording. The recording's own credit row stays the combined credit.
CREATE TABLE account_recording_credit_members (
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    artist_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK(role IN ('main','featured')),
    PRIMARY KEY(user_id,recording_id,artist_id),
    FOREIGN KEY(user_id,artist_id) REFERENCES account_artists(user_id,id)
);
CREATE INDEX account_recording_credit_member_artist ON account_recording_credit_members(user_id,artist_id,role,recording_id);
CREATE INDEX account_recording_credit_member_recording ON account_recording_credit_members(recording_id,user_id);
