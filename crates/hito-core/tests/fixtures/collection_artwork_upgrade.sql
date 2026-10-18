INSERT INTO users(id,username,password_hash) VALUES ('artwork-other','artwork-other','unused-owned-fixture-hash');
INSERT INTO libraries(id,name,owner_id) VALUES
    ('artwork-ready-library','Ready private library','legacy-admin'),
    ('artwork-pending-library','Pending private library','artwork-other');
INSERT INTO library_members(library_id,user_id) VALUES
    ('artwork-ready-library','legacy-admin'),('artwork-pending-library','artwork-other');
INSERT INTO plugin_installations(id,user_id,service_id,manifest) VALUES
    ('artwork-ready-installation','legacy-admin','artwork-upgrade-service',
        '{"id":"org.hito.owned-collection-artwork","name":"Owned collection artwork","protocol_version":1,"capabilities":["playlists","artwork"]}'),
    ('artwork-pending-installation','artwork-other','artwork-upgrade-service',
        '{"id":"org.hito.owned-collection-artwork","name":"Owned collection artwork","protocol_version":1,"capabilities":["playlists","artwork"]}');
INSERT INTO recordings(rowid,id,library_id,title,artist,duration_ms,provenance) VALUES
    (701,'20000000-0000-4000-8000-000000000001','artwork-ready-library','Observed ready track','Observed performer',123000,'provider_response'),
    (1907,'20000000-0000-4000-8000-000000000002','artwork-pending-library','Observed pending track','Observed performer',123000,'provider_response');
INSERT INTO provider_items(id,installation_id,provider_recording_id,recording_id,role,evidence) VALUES
    ('artwork-ready-binding','artwork-ready-installation','recording/owned-artwork','20000000-0000-4000-8000-000000000001','primary','provider_identity'),
    ('artwork-pending-binding','artwork-pending-installation','recording/owned-artwork','20000000-0000-4000-8000-000000000002','primary','provider_identity');
INSERT INTO metadata_observations(id,recording_id,provider,origin,observed_at,title,artist,duration_ms,
    plugin_installation_id,provider_recording_id,provider_metadata,provider_digest,provider_binding_id) VALUES
    ('artwork-ready-observation','20000000-0000-4000-8000-000000000001','org.hito.owned-collection-artwork','provider_response',123456,
        'Observed ready track','Observed performer',123000,'artwork-ready-installation','recording/owned-artwork',
        '{"id":"recording/owned-artwork","title":"Observed ready track","artist":"Observed performer","duration_ms":123000}',
        'owned-ready-digest','artwork-ready-binding'),
    ('artwork-pending-observation','20000000-0000-4000-8000-000000000002','org.hito.owned-collection-artwork','provider_response',123457,
        'Observed pending track','Observed performer',123000,'artwork-pending-installation','recording/owned-artwork',
        '{"id":"recording/owned-artwork","title":"Observed pending track","artist":"Observed performer","duration_ms":123000}',
        'owned-pending-digest','artwork-pending-binding');
UPDATE recordings SET metadata_binding_id='artwork-ready-binding',metadata_observation_id='artwork-ready-observation'
    WHERE id='20000000-0000-4000-8000-000000000001';
UPDATE recordings SET metadata_binding_id='artwork-pending-binding',metadata_observation_id='artwork-pending-observation'
    WHERE id='20000000-0000-4000-8000-000000000002';
INSERT INTO personal_recording_metadata(user_id,recording_id,override_title,override_artist,title,artist,title_operation_id,artist_operation_id) VALUES
    ('legacy-admin','20000000-0000-4000-8000-000000000001','My ready title','My ready artist','My ready title','My ready artist','owned-ready-title-operation','owned-ready-artist-operation'),
    ('artwork-other','20000000-0000-4000-8000-000000000002','My pending title','My pending artist','My pending title','My pending artist','owned-pending-title-operation','owned-pending-artist-operation');
INSERT INTO likes(user_id,recording_id,liked) VALUES
    ('legacy-admin','20000000-0000-4000-8000-000000000001',1),('artwork-other','20000000-0000-4000-8000-000000000002',1);
INSERT INTO listening_progress(user_id,recording_id,position_ms,speed,updated_at) VALUES
    ('legacy-admin','20000000-0000-4000-8000-000000000001',4200,1.25,123456),
    ('artwork-other','20000000-0000-4000-8000-000000000002',7300,1.5,123457);
INSERT INTO playlists(id,user_id,name) VALUES ('30000000-0000-4000-8000-000000000001','legacy-admin','My historical private copy');
INSERT INTO playlist_entries(playlist_id,id,recording_id,position) VALUES
    ('30000000-0000-4000-8000-000000000001','40000000-0000-4000-8000-000000000001','20000000-0000-4000-8000-000000000001',0),
    ('30000000-0000-4000-8000-000000000001','40000000-0000-4000-8000-000000000002','20000000-0000-4000-8000-000000000001',1);
