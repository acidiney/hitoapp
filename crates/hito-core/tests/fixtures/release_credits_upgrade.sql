INSERT INTO libraries(id,name,owner_id) VALUES
    ('30000000-0000-4000-8000-000000000002','Provider library','legacy-admin');
INSERT INTO library_members(library_id,user_id) VALUES
    ('30000000-0000-4000-8000-000000000002','legacy-admin');
INSERT INTO plugin_installations(id,user_id,service_id,manifest) VALUES
    ('release-credit-installation','legacy-admin','release-credit-service',
     '{"id":"org.hito.content-fixture","name":"Owned provider","protocol_version":1,"capabilities":["sources","playlists"]}');
INSERT INTO releases(rowid,id,library_id,title,artist,identity_key) VALUES
    (811,'30000000-0000-4000-8000-000000000004','30000000-0000-4000-8000-000000000002',
     'Provider album','Provider album credit','["plugin","release-credit-installation","album/known"]');
INSERT INTO recordings(rowid,id,library_id,release_id,title,artist,duration_ms,provenance) VALUES
    (1211,'30000000-0000-4000-8000-000000000003','30000000-0000-4000-8000-000000000002',
     '30000000-0000-4000-8000-000000000004','Provider track','Provider performer',123000,'provider_response');
INSERT INTO provider_items(installation_id,provider_recording_id,recording_id) VALUES
    ('release-credit-installation','recording/known','30000000-0000-4000-8000-000000000003');
INSERT INTO sources(rowid,id,recording_id,provider,provider_id,ready) VALUES
    (1511,'30000000-0000-4000-8000-000000000005','30000000-0000-4000-8000-000000000003','plugin','audio/known',1);
INSERT INTO provider_sources(source_id,installation_id,recording_id,provider_source_id,label,generation) VALUES
    ('30000000-0000-4000-8000-000000000005','release-credit-installation',
     '30000000-0000-4000-8000-000000000003','audio/known','Owned source',3);
INSERT INTO account_artists(user_id,id,library_id,name) VALUES
    ('legacy-admin','private-local-credit','10000000-0000-4000-8000-000000000003','Private local credit'),
    ('legacy-admin','provider-performer','30000000-0000-4000-8000-000000000002','Provider performer'),
    ('legacy-admin','private-provider-credit','30000000-0000-4000-8000-000000000002','Private provider credit'),
    ('legacy-admin','private-provider-performer','30000000-0000-4000-8000-000000000002','Private performer');
INSERT INTO personal_release_metadata(user_id,release_id,override_title,override_artist,title_operation_id,artist_operation_id) VALUES
    ('legacy-admin','30000000-0000-4000-8000-000000000001','Pinned local album','Private local credit','local-title-operation','local-artist-operation'),
    ('legacy-admin','30000000-0000-4000-8000-000000000004','Pinned provider album','Private provider credit','provider-title-operation','provider-artist-operation');
UPDATE account_releases SET rowid=1201,title='Pinned local album',artist='Private local credit',artist_id='private-local-credit'
    WHERE id='30000000-0000-4000-8000-000000000001';
INSERT INTO account_releases(rowid,user_id,id,library_id,title,artist,artist_id) VALUES
    (1601,'legacy-admin','30000000-0000-4000-8000-000000000004','30000000-0000-4000-8000-000000000002',
     'Pinned provider album','Private provider credit','private-provider-credit');
INSERT INTO personal_recording_metadata(user_id,recording_id,override_title,override_artist,title,artist,title_operation_id,artist_operation_id) VALUES
    ('legacy-admin','30000000-0000-4000-8000-000000000003','Pinned recording','Private performer',
     'Pinned recording','Private performer','recording-title-operation','recording-artist-operation');
INSERT INTO account_recording_credits(user_id,recording_id,artist_id) VALUES
    ('legacy-admin','30000000-0000-4000-8000-000000000003','private-provider-performer');
INSERT INTO likes(user_id,recording_id,liked) VALUES
    ('legacy-admin','30000000-0000-4000-8000-000000000003',1);
INSERT INTO listening_progress(user_id,recording_id,position_ms,speed,updated_at) VALUES
    ('legacy-admin','30000000-0000-4000-8000-000000000003',4321,1.25,123456);
INSERT INTO playlists(id,user_id,name,kind) VALUES
    ('30000000-0000-4000-8000-000000000006','legacy-admin','Provider copy','local');
INSERT INTO playlist_entries(playlist_id,id,recording_id,position) VALUES
    ('10000000-0000-4000-8000-000000000004','30000000-0000-4000-8000-000000000007','30000000-0000-4000-8000-000000000003',1),
    ('30000000-0000-4000-8000-000000000006','30000000-0000-4000-8000-000000000008','30000000-0000-4000-8000-000000000003',0),
    ('30000000-0000-4000-8000-000000000006','30000000-0000-4000-8000-000000000009','30000000-0000-4000-8000-000000000003',1);
INSERT INTO playlist_imports(playlist_id,installation_id,installation_revision,service_id,service_revision,provider,provider_playlist_id,snapshot,attribution,imported_at) VALUES
    ('30000000-0000-4000-8000-000000000006','release-credit-installation',1,'release-credit-service',1,
     'org.hito.content-fixture','playlist/known','known-one','{"label":"Owned provider"}',123456);
INSERT INTO playlist_import_entries(playlist_id,entry_id,occurrence_id,provider_recording_id,kind) VALUES
    ('30000000-0000-4000-8000-000000000006','30000000-0000-4000-8000-000000000008','first','recording/known','recording'),
    ('30000000-0000-4000-8000-000000000006','30000000-0000-4000-8000-000000000009','again','recording/known','recording');
