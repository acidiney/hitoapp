CREATE TEMP TABLE podcast_archive_numbers(i INTEGER PRIMARY KEY);
WITH RECURSIVE n(i) AS (
    VALUES(0) UNION ALL SELECT i+1 FROM n WHERE i<99999
)
INSERT INTO podcast_archive_numbers SELECT i FROM n;

INSERT INTO recordings(id,library_id,title,artist,duration_ms,provenance)
SELECT printf('a1000000-0000-4000-8000-%012x',n.i),
    fixture.library_id,printf('Archive episode %06d',n.i),
    'Owned archive author',1000,'rss_feed'
FROM podcast_archive_numbers n CROSS JOIN podcast_archive_fixture fixture
ORDER BY n.i DESC;

INSERT INTO account_artists(user_id,id,library_id,name)
SELECT user_id,'a3000000-0000-4000-8000-000000000001',library_id,'Owned archive author'
FROM podcast_archive_fixture;

INSERT INTO account_recording_credits(user_id,recording_id,artist_id)
SELECT fixture.user_id,printf('a1000000-0000-4000-8000-%012x',n.i),
    'a3000000-0000-4000-8000-000000000001'
FROM podcast_archive_numbers n CROSS JOIN podcast_archive_fixture fixture;

INSERT INTO podcast_episodes(
    id,subscription_id,user_id,recording_id,identity_kind,identity_value,
    enclosure_url,declared_mime,byte_length,published_at)
SELECT printf('a2000000-0000-4000-8000-%012x',n.i),
    fixture.subscription_id,fixture.user_id,
    printf('a1000000-0000-4000-8000-%012x',n.i),
    'guid',printf('archive-opaque-%06d',n.i),
    'https://owned-archive.invalid/audio.wav','audio/wav',16044,
    CASE WHEN n.i<60000 THEN 1700000000-n.i/3 ELSE NULL END
FROM podcast_archive_numbers n CROSS JOIN podcast_archive_fixture fixture
ORDER BY n.i DESC;

UPDATE podcast_subscriptions SET content_revision=1
WHERE id=(SELECT subscription_id FROM podcast_archive_fixture);

DROP TABLE podcast_archive_numbers;
DROP TABLE podcast_archive_fixture;
