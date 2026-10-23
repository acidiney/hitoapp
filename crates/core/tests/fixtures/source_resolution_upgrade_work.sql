INSERT INTO recordings(rowid,id,library_id,title,artist,duration_ms,provenance)
SELECT 10000+n.i*3,'work-recording-'||n.i,'migration-library','Owned scale track','Owned scale performer',12000,'provider_response'
FROM hito_resolution_work_numbers n;
INSERT INTO provider_items(rowid,id,installation_id,provider_recording_id,recording_id,role,evidence,created_at)
SELECT 20000+n.i*3,'work-binding-'||n.i,'migration-installation','owned-work-'||n.i,'work-recording-'||n.i,'primary','provider_identity',123
FROM hito_resolution_work_numbers n;
INSERT INTO metadata_observations(id,recording_id,provider,origin,title,artist,duration_ms,
    plugin_installation_id,provider_recording_id,provider_metadata,provider_digest,provider_binding_id)
SELECT 'work-observation-'||n.i,'work-recording-'||n.i,'org.hito.migration-fixture','provider_response','Owned scale track','Owned scale performer',12000,
    'migration-installation','owned-work-'||n.i,json_object('id','owned-work-'||n.i,'title','Owned scale track','artist','Owned scale performer','duration_ms',12000),'owned-work-digest','work-binding-'||n.i
FROM hito_resolution_work_numbers n;
UPDATE recordings SET metadata_binding_id='work-binding-'||substr(id,16),metadata_observation_id='work-observation-'||substr(id,16)
WHERE id GLOB 'work-recording-*';
INSERT INTO provider_source_state(binding_id,generation,observation_id)
SELECT 'work-binding-'||n.i,1,'work-observation-'||n.i FROM hito_resolution_work_numbers n;
INSERT INTO sources(id,recording_id,provider,provider_id,ready)
SELECT 'work-source-'||n.i,'work-recording-'||n.i,'plugin',json_array('work-binding-'||n.i,'owned-work-source-'||n.i),1
FROM hito_resolution_work_numbers n;
INSERT INTO provider_sources(source_id,binding_id,installation_id,recording_id,provider_source_id,label,generation)
SELECT 'work-source-'||n.i,'work-binding-'||n.i,'migration-installation','work-recording-'||n.i,'owned-work-source-'||n.i,'Owned scale source',1
FROM hito_resolution_work_numbers n;
INSERT INTO artwork_fetches(id,user_id,recording_id,session_hash,observation_id,binding_id,binding_revision,
    installation_id,installation_revision,service_id,service_revision,status,reservation_bytes)
SELECT 'work-artwork-'||n.i,'migration-owner','work-recording-'||n.i,'owned-migration-session','work-observation-'||n.i,'work-binding-'||n.i,1,
    'migration-installation',1,'migration-service',1,'failed',0
FROM hito_resolution_work_numbers n;
INSERT INTO source_preparations(id,user_id,recording_id,source_id,library_id,access_grant,binding_id,binding_revision,provider_recording_id,remote_source_id,
    installation_id,installation_revision,service_id,service_revision,provider,credential,audio_generation,phase,submission_state,remote_outcome,remote_handle,next_check_at)
SELECT 'work-preparation-'||n.i,'migration-owner','work-recording-'||n.i,'work-source-'||n.i,'migration-library',access.id,'work-binding-'||n.i,1,'owned-work-'||n.i,'owned-work-source-'||n.i,
    'migration-installation',1,'migration-service',1,'org.hito.migration-fixture',X'0100FE02',0,'failed','submitted','failed','owned-work-handle-'||n.i,NULL
FROM hito_resolution_work_numbers n CROSS JOIN library_access_grants access WHERE access.library_id='migration-library' AND access.user_id='migration-owner';
INSERT INTO download_intents(id,account_id,recording_id,library_id,access_grant_id,host_device,generation,state,provider_source_id,binding_id,binding_revision,remote_source_id,
    installation_id,installation_revision,service_id,service_revision,credential_digest,audio_generation)
SELECT 'work-download-'||n.i,'migration-owner','work-recording-'||n.i,'migration-library',access.id,'migration-host',2,'removed','work-source-'||n.i,'work-binding-'||n.i,1,'owned-work-source-'||n.i,
    'migration-installation',1,'migration-service',1,'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',0
FROM hito_resolution_work_numbers n CROSS JOIN library_access_grants access WHERE access.library_id='migration-library' AND access.user_id='migration-owner';
