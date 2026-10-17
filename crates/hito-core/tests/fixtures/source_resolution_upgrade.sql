INSERT INTO users(id,username,password_hash,admin) VALUES
    ('migration-owner','owner','unused-owned-fixture-hash',1),
    ('migration-other','other','unused-owned-fixture-hash',0);
INSERT INTO sessions(token_hash,user_id,expires_at) VALUES ('owned-migration-session','migration-owner',4102444800);
INSERT INTO libraries(id,name,owner_id) VALUES
    ('migration-library','Owned migration library','migration-owner'),
    ('migration-other-library','Other account library','migration-other');
INSERT INTO library_members(library_id,user_id) VALUES ('migration-library','migration-owner'),('migration-other-library','migration-other');
INSERT INTO plugin_services(id,name,base_url) VALUES ('migration-service','Owned migration service','http://127.0.0.1:9/');
INSERT INTO plugin_installations(id,user_id,service_id,manifest,credential) VALUES
    ('migration-installation','migration-owner','migration-service',
    '{"id":"org.hito.migration-fixture","name":"Owned migration provider","protocol_version":1,"capabilities":["sources","search","artwork","source_preparation"]}',X'0100FE02');
INSERT INTO releases(id,library_id,title,artist,identity_key) VALUES ('migration-release','migration-library','Owned album','Owned artist','owned-album');
INSERT INTO recordings(rowid,id,library_id,release_id,title,artist,duration_ms,provenance) VALUES
    (901,'migration-recording','migration-library','migration-release','Owned track','Owned artist',12000,'provider_response'),
    (1047,'migration-other-recording','migration-other-library',NULL,'Other track','Other artist',15000,'provider_response');
INSERT INTO provider_items(rowid,id,installation_id,provider_recording_id,recording_id,role,evidence,revision,retired,created_at) VALUES
    (507,'migration-primary','migration-installation','owned-primary','migration-recording','primary','provider_identity',1,0,123),
    (812,'migration-manual','migration-installation','owned-manual','migration-recording','source','user_confirmed',1,0,124),
    (1907,'migration-retired','migration-installation','owned-retired','migration-recording','source','user_confirmed',2,1,125);
INSERT INTO metadata_observations(sequence,id,recording_id,release_id,provider,origin,observed_at,title,artist,album,duration_ms,
    plugin_installation_id,provider_recording_id,provider_metadata,provider_digest,provider_binding_id) VALUES
    (7,'migration-observation-primary','migration-recording','migration-release','org.hito.migration-fixture','provider_response',200,'Owned track','Owned artist','Owned album',12000,
        'migration-installation','owned-primary','{"id":"owned-primary","title":"Owned track","artist":"Owned artist","duration_ms":12000}','primary-digest','migration-primary'),
    (97,'migration-observation-manual','migration-recording','migration-release','org.hito.migration-fixture','provider_response',201,'Reviewed track','Owned artist','Owned album',12000,
        'migration-installation','owned-manual','{"id":"owned-manual","title":"Reviewed track","artist":"Owned artist","duration_ms":12000}','manual-digest','migration-manual'),
    (303,'migration-observation-retired','migration-recording','migration-release','org.hito.migration-fixture','provider_response',202,'Retained track','Owned artist','Owned album',12000,
        'migration-installation','owned-retired','{"id":"owned-retired","title":"Retained track","artist":"Owned artist","duration_ms":12000}','retired-digest','migration-retired');
UPDATE recordings SET metadata_binding_id='migration-primary',metadata_observation_id='migration-observation-primary' WHERE id='migration-recording';
INSERT INTO metadata_observations(sequence,id,recording_id,provider,origin,title,artist,duration_ms)
    VALUES (900007,'migration-deleted-high-water','migration-recording','owned','provider_response','Deleted owned observation','Owned artist',12000);
DELETE FROM metadata_observations WHERE id='migration-deleted-high-water';
INSERT INTO provider_source_state(binding_id,generation,observation_id) VALUES
    ('migration-primary',2,'migration-observation-primary'),('migration-manual',4,'migration-observation-manual'),('migration-retired',1,'migration-observation-retired');
INSERT INTO sources(rowid,id,recording_id,provider,provider_id,device_id,ready) VALUES
    (102,'migration-source-primary','migration-recording','plugin','owned-source-primary',NULL,0),
    (407,'migration-source-manual','migration-recording','plugin','owned-source-manual',NULL,1),
    (901,'migration-source-retired','migration-recording','plugin','owned-source-retired',NULL,0),
    (1231,'migration-source-saved','migration-recording','download','owned-saved-source','migration-host',1);
INSERT INTO provider_sources(source_id,binding_id,installation_id,recording_id,provider_source_id,label,generation) VALUES
    ('migration-source-primary','migration-primary','migration-installation','migration-recording','owned-source-primary','Owned unavailable source',2),
    ('migration-source-manual','migration-manual','migration-installation','migration-recording','owned-source-manual','Reviewed source',4),
    ('migration-source-retired','migration-retired','migration-installation','migration-recording','owned-source-retired','Retained source',1);
INSERT INTO source_metadata(source_id,release_id,title,artist,duration_ms,observation_id,provenance) VALUES
    ('migration-source-primary','migration-release','Owned track','Owned artist',12000,'migration-observation-primary','provider_response'),
    ('migration-source-manual','migration-release','Reviewed track','Owned artist',12000,'migration-observation-manual','provider_response'),
    ('migration-source-retired','migration-release','Retained track','Owned artist',12000,'migration-observation-retired','provider_response');
INSERT INTO personal_recording_metadata(user_id,recording_id,override_title,override_artist,title,artist,title_operation_id,artist_operation_id)
    VALUES ('migration-owner','migration-recording','Private track','Private artist','Private track','Private artist','owned-title-operation','owned-artist-operation');
INSERT INTO likes(user_id,recording_id,liked) VALUES ('migration-owner','migration-recording',1);
INSERT INTO listening_progress(user_id,recording_id,position_ms,speed,updated_at) VALUES ('migration-owner','migration-recording',6731,1.25,300);
INSERT INTO playlists(id,user_id,name) VALUES ('migration-playlist','migration-owner','Owned playlist');
INSERT INTO playlist_entries(playlist_id,id,recording_id,position) VALUES
    ('migration-playlist','migration-entry-one','migration-recording',0),('migration-playlist','migration-entry-two','migration-recording',1);
INSERT INTO artwork_fetches(id,user_id,recording_id,session_hash,observation_id,binding_id,binding_revision,installation_id,installation_revision,service_id,service_revision,status,reservation_bytes)
    VALUES ('migration-artwork-fetch','migration-owner','migration-recording','owned-migration-session','migration-observation-primary','migration-primary',1,'migration-installation',1,'migration-service',1,'ready',0);
INSERT INTO artwork_assets(user_id,content_hash,content_type,width,height,bytes)
    VALUES ('migration-owner','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','image/png',1,1,X'89504E470D0A1A0A');
INSERT INTO recording_artwork(user_id,recording_id,observation_id,fetch_id,content_hash)
    VALUES ('migration-owner','migration-recording','migration-observation-primary','migration-artwork-fetch','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa');
INSERT INTO download_files(id,account_id,host_device,recording_id,source_id,state,attempt_id,checksum,byte_length,mime)
    VALUES ('migration-download-file','migration-owner','migration-host','migration-recording','migration-source-saved','ready','owned-attempt','bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',44,'audio/wav');
INSERT INTO download_intents(rowid,id,account_id,recording_id,library_id,access_grant_id,host_device,state,file_id,provider_source_id,binding_id,binding_revision,remote_source_id,
    installation_id,installation_revision,service_id,service_revision,credential_digest,audio_generation)
    SELECT 307,'migration-download-intent','migration-owner','migration-recording','migration-library',id,'migration-host','queued','migration-download-file',
        'migration-source-manual','migration-manual',1,'owned-source-manual','migration-installation',1,'migration-service',1,'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',0
    FROM library_access_grants WHERE library_id='migration-library' AND user_id='migration-owner';
INSERT INTO download_retentions(id,account_id,host_device,kind,recording_id,access_grant_id,transfer_id,state)
    SELECT 'migration-download-retention','migration-owner','migration-host','recording','migration-recording',id,'migration-download-intent','active'
    FROM library_access_grants WHERE library_id='migration-library' AND user_id='migration-owner';
INSERT INTO download_receipts(account_id,request_id,request_digest,intent_id,retention_id)
    VALUES ('migration-owner','owned-download-request','dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd','migration-download-intent','migration-download-retention');
INSERT INTO download_jobs(id,account_id,intent_id,generation,status)
    VALUES ('migration-download-job','migration-owner','migration-download-intent',1,'queued');
INSERT INTO source_preparations(rowid,id,user_id,recording_id,source_id,library_id,access_grant,binding_id,binding_revision,provider_recording_id,remote_source_id,
    installation_id,installation_revision,service_id,service_revision,provider,credential,audio_generation)
    SELECT 713,'migration-preparation','migration-owner','migration-recording','migration-source-primary','migration-library',id,'migration-primary',1,'owned-primary','owned-source-primary',
        'migration-installation',1,'migration-service',1,'org.hito.migration-fixture',X'0100FE02',0
    FROM library_access_grants WHERE library_id='migration-library' AND user_id='migration-owner';
INSERT INTO source_preparation_receipts(user_id,request_id,request_digest,preparation_id)
    VALUES ('migration-owner','owned-preparation-request','eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee','migration-preparation');
