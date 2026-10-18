INSERT INTO releases(rowid,id,library_id,title,artist,identity_key)
SELECT 100000+n.i*7,'release-work-album-'||n.i,'release-work-library',
    'Owned scale album '||printf('%06d',n.i),'Owned scale album credit','release-work-identity-'||n.i
FROM hito_release_credit_work_numbers n;

INSERT INTO recordings(rowid,id,library_id,release_id,title,artist,duration_ms)
SELECT 300000+n.i*7,'release-work-recording-'||n.i,'release-work-library','release-work-album-'||n.i,
    'Owned scale track '||n.i,'Owned scale performer',12000
FROM hito_release_credit_work_numbers n;

INSERT INTO sources(rowid,id,recording_id,provider,provider_id,device_id,ready)
SELECT 400000+n.i*7,'release-work-source-'||n.i,'release-work-recording-'||n.i,
    'local','release-work-locator-'||n.i,'release-credit-work-host',0
FROM hito_release_credit_work_numbers n;

INSERT INTO source_metadata(source_id,release_id,title,artist,duration_ms,disc_number,track_number)
SELECT 'release-work-source-'||n.i,'release-work-album-'||n.i,
    'Owned scale track '||n.i,'Owned scale performer',12000,1,1
FROM hito_release_credit_work_numbers n;

UPDATE recordings SET metadata_source_id='release-work-source-'||substr(id,24)
WHERE id IN (SELECT 'release-work-recording-'||i FROM hito_release_credit_work_numbers);

INSERT INTO source_artist_credits(source_id,artist_id)
SELECT 'release-work-source-'||n.i,artist.id
FROM hito_release_credit_work_numbers n
CROSS JOIN artists artist ON artist.library_id='release-work-library' AND artist.name='Owned scale performer';

INSERT INTO account_artists(user_id,id,library_id,name)
SELECT account.user_id,artist.id,artist.library_id,artist.name
FROM hito_release_credit_work_accounts account
CROSS JOIN artists artist ON artist.library_id='release-work-library' AND artist.name='Owned scale performer'
WHERE 1 ON CONFLICT(user_id,library_id,name) DO NOTHING;

INSERT INTO personal_release_metadata(rowid,user_id,release_id,override_title,override_artist,title_operation_id,artist_operation_id)
SELECT 600000+n.i*101+account.position,account.user_id,'release-work-album-'||n.i,
    'Private scale album '||printf('%06d',n.i),account.credit,
    'release-work-title-'||n.i||'-'||account.position,'release-work-credit-'||n.i||'-'||account.position
FROM hito_release_credit_work_numbers n CROSS JOIN hito_release_credit_work_accounts account;

INSERT INTO account_releases(rowid,user_id,id,library_id,title,artist,artist_id)
SELECT 200000+n.i*101+account.position,account.user_id,'release-work-album-'||n.i,'release-work-library',
    'Private scale album '||printf('%06d',n.i),account.credit,account.artist_id
FROM hito_release_credit_work_numbers n CROSS JOIN hito_release_credit_work_accounts account;

INSERT INTO account_source_credits(rowid,user_id,source_id,recording_id,release_id,artist_id)
SELECT 700000+n.i*101+account.position,account.user_id,'release-work-source-'||n.i,
    'release-work-recording-'||n.i,'release-work-album-'||n.i,artist.id
FROM hito_release_credit_work_numbers n CROSS JOIN hito_release_credit_work_accounts account
CROSS JOIN account_artists artist ON artist.user_id=account.user_id
    AND artist.library_id='release-work-library' AND artist.name='Owned scale performer';

INSERT INTO account_recording_credits(rowid,user_id,recording_id,artist_id)
SELECT 800000+n.i*101+account.position,account.user_id,'release-work-recording-'||n.i,artist.id
FROM hito_release_credit_work_numbers n CROSS JOIN hito_release_credit_work_accounts account
CROSS JOIN account_artists artist ON artist.user_id=account.user_id
    AND artist.library_id='release-work-library' AND artist.name='Owned scale performer';
