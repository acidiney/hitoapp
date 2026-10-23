INSERT INTO users(rowid,id,username,password_hash,admin,created_at)
VALUES(71,'podcast-migration-owner','podcast-migration-owner','unused-owned-fixture-hash',1,1700000000);
INSERT INTO libraries(rowid,id,name,owner_id)
VALUES(83,'41000000-0000-4000-8000-000000000001','Owned podcast library','podcast-migration-owner');
INSERT INTO library_members(library_id,user_id)
VALUES('41000000-0000-4000-8000-000000000001','podcast-migration-owner');

INSERT INTO recordings(rowid,id,library_id,title,artist,duration_ms,provenance) VALUES
    (103,'42000000-0000-4000-8000-000000000001','41000000-0000-4000-8000-000000000001','First inherited episode','Owned inherited author',12000,'rss_feed'),
    (307,'42000000-0000-4000-8000-000000000002','41000000-0000-4000-8000-000000000001','Second inherited episode','Owned inherited author',18000,'rss_feed');
INSERT INTO sources(rowid,id,recording_id,provider,provider_id,device_id,ready)
VALUES(503,'43000000-0000-4000-8000-000000000001','42000000-0000-4000-8000-000000000001','local','owned-retained-local-source','retained-host',0);

INSERT INTO personal_recording_metadata(id,user_id,recording_id,override_title,override_artist,title,artist,title_operation_id,artist_operation_id) VALUES
    (701,'podcast-migration-owner','42000000-0000-4000-8000-000000000001','First private episode','First private author','First private episode','First private author','owned-title-one','owned-artist-one'),
    (907,'podcast-migration-owner','42000000-0000-4000-8000-000000000002','Second private episode','Second private author','Second private episode','Second private author','owned-title-two','owned-artist-two');
INSERT INTO account_artists(user_id,id,library_id,name) VALUES
    ('podcast-migration-owner','44000000-0000-4000-8000-000000000001','41000000-0000-4000-8000-000000000001','First private author'),
    ('podcast-migration-owner','44000000-0000-4000-8000-000000000002','41000000-0000-4000-8000-000000000001','Second private author');
INSERT INTO account_recording_credits(user_id,recording_id,artist_id) VALUES
    ('podcast-migration-owner','42000000-0000-4000-8000-000000000001','44000000-0000-4000-8000-000000000001'),
    ('podcast-migration-owner','42000000-0000-4000-8000-000000000002','44000000-0000-4000-8000-000000000002');
INSERT INTO likes(user_id,recording_id,liked) VALUES
    ('podcast-migration-owner','42000000-0000-4000-8000-000000000001',1),
    ('podcast-migration-owner','42000000-0000-4000-8000-000000000002',0);
INSERT INTO listening_progress(user_id,recording_id,position_ms,speed,updated_at) VALUES
    ('podcast-migration-owner','42000000-0000-4000-8000-000000000001',700,1.5,1700000001),
    ('podcast-migration-owner','42000000-0000-4000-8000-000000000002',900,1.25,1700000002);
INSERT INTO playlists(rowid,id,user_id,name)
VALUES(1103,'45000000-0000-4000-8000-000000000001','podcast-migration-owner','Owned duplicate episode playlist');
INSERT INTO playlist_entries(playlist_id,id,recording_id,position) VALUES
    ('45000000-0000-4000-8000-000000000001','46000000-0000-4000-8000-000000000001','42000000-0000-4000-8000-000000000001',0),
    ('45000000-0000-4000-8000-000000000001','46000000-0000-4000-8000-000000000002','42000000-0000-4000-8000-000000000001',1);

INSERT INTO podcast_subscriptions(rowid,id,user_id,library_id,access_grant_id,feed_url,fetch_url,title,author,description,content_revision,last_refreshed_at,next_refresh_at,created_at)
SELECT 1301,'47000000-0000-4000-8000-000000000001',user_id,library_id,id,
    'https://owned-podcast-migration.invalid/feed.xml','https://owned-podcast-migration.invalid/feed.xml',
    'Owned retained podcast','Owned inherited author','Owned feed description',7,1700000010,4102444800,1700000000
FROM library_access_grants WHERE user_id='podcast-migration-owner';
INSERT INTO podcast_episodes(rowid,id,subscription_id,user_id,recording_id,identity_kind,identity_value,enclosure_url,enclosure_revision,declared_mime,byte_length,description,published_at,first_seen_at,last_seen_at) VALUES
    (1601,'48000000-0000-4000-8000-000000000001','47000000-0000-4000-8000-000000000001','podcast-migration-owner','42000000-0000-4000-8000-000000000001','guid','owned-opaque-first-guid','https://owned-podcast-migration.invalid/first.wav?owned-private-canary=one',3,'audio/wav',16044,'First owned description',1700000000,1700000000,1700000010),
    (1907,'48000000-0000-4000-8000-000000000002','47000000-0000-4000-8000-000000000001','podcast-migration-owner','42000000-0000-4000-8000-000000000002','guid','owned-opaque-second-guid','https://owned-podcast-migration.invalid/second.wav?owned-private-canary=two',2,'audio/wav',32044,'Second owned description',NULL,1700000000,1700000010);
INSERT INTO podcast_refresh_jobs(rowid,id,subscription_id,user_id,generation,status,attempts,created_at,updated_at)
VALUES(2203,'49000000-0000-4000-8000-000000000001','47000000-0000-4000-8000-000000000001','podcast-migration-owner',1,'complete',1,1700000000,1700000010);
