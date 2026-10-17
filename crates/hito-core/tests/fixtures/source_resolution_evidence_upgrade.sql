INSERT INTO recordings(rowid,id,library_id,title,artist,duration_ms,provenance) VALUES
    (3029,'evidence-recording','migration-library','Owned evidence track','Owned evidence artist',12000,'provider_response');
INSERT INTO provider_items(rowid,id,installation_id,provider_recording_id,recording_id,role,evidence,revision,retired,created_at) VALUES
    (4093,'evidence-primary','migration-installation','owned-evidence-primary','evidence-recording','primary','provider_identity',1,0,1200),
    (6101,'evidence-automatic','migration-installation','owned-evidence-automatic','evidence-recording','source','automatic_policy',1,0,1210);
INSERT INTO metadata_observations(id,recording_id,provider,origin,observed_at,title,artist,duration_ms,
    plugin_installation_id,provider_recording_id,provider_metadata,provider_digest,provider_binding_id) VALUES
    ('evidence-primary-observation','evidence-recording','org.hito.migration-fixture','provider_response',1201,'Owned evidence track','Owned evidence artist',12000,
        'migration-installation','owned-evidence-primary','{"id":"owned-evidence-primary","title":"Owned evidence track","artist":"Owned evidence artist","duration_ms":12000}','owned-evidence-primary-digest','evidence-primary'),
    ('evidence-automatic-observation','evidence-recording','org.hito.migration-fixture','provider_response',1211,'Owned evidence track','Owned evidence artist',12000,
        'migration-installation','owned-evidence-automatic','{"id":"owned-evidence-automatic","title":"Owned evidence track","artist":"Owned evidence artist","duration_ms":12000}','owned-evidence-automatic-digest','evidence-automatic');
UPDATE recordings SET metadata_binding_id='evidence-primary',metadata_observation_id='evidence-primary-observation'
    WHERE id='evidence-recording';
INSERT INTO provider_source_state(binding_id,generation,observation_id) VALUES
    ('evidence-primary',1,'evidence-primary-observation'),('evidence-automatic',1,'evidence-automatic-observation');
INSERT INTO sources(rowid,id,recording_id,provider,provider_id,ready) VALUES
    (2027,'evidence-source','evidence-recording','plugin','["evidence-automatic","owned-evidence-source"]',1);
INSERT INTO provider_sources(source_id,binding_id,installation_id,recording_id,provider_source_id,label,generation) VALUES
    ('evidence-source','evidence-automatic','migration-installation','evidence-recording','owned-evidence-source','Owned evidence source',1);
INSERT INTO source_metadata(source_id,title,artist,duration_ms,observation_id,provenance) VALUES
    ('evidence-source','Owned evidence track','Owned evidence artist',12000,'evidence-automatic-observation','provider_response');
INSERT INTO recording_source_choices(rowid,user_id,recording_id,revision) VALUES
    (2003,'migration-owner','evidence-recording',7);
INSERT INTO source_resolutions(rowid,id,user_id,recording_id,target_digest,choice_revision,outcome,lease_token,lease_until,created_at,completed_at) VALUES
    (2011,'evidence-resolution','migration-owner','evidence-recording',
        '1111111111111111111111111111111111111111111111111111111111111111',7,'ready',NULL,NULL,1205,1215);
INSERT INTO source_resolution_receipts(rowid,user_id,request_id,recording_id,resolution_id,created_at) VALUES
    (2039,'migration-owner','owned-evidence-request','evidence-recording','evidence-resolution',1206);
INSERT INTO automatic_source_decisions(rowid,resolution_id,binding_id,policy_version,target_digest,candidate_digest,installation_revision,service_revision,created_at) VALUES
    (2053,'evidence-resolution','evidence-automatic','hito.musical-source.v1',
        '1111111111111111111111111111111111111111111111111111111111111111',
        '2222222222222222222222222222222222222222222222222222222222222222',1,1,1215);
