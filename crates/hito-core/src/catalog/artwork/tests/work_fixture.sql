INSERT INTO recordings(id,library_id,release_id,title,artist,duration_ms,provenance)
SELECT printf('af100000-0000-4000-8000-%012x',i),r.library_id,r.release_id,r.title,r.artist,r.duration_ms,r.provenance
FROM artwork_work_indices CROSS JOIN artwork_work_context template
JOIN artwork_fetches j ON j.id=template.fetch_id JOIN recordings r ON r.id=j.recording_id;

INSERT INTO provider_items(id,installation_id,provider_recording_id,recording_id,role,evidence)
SELECT printf('af200000-0000-4000-8000-%012x',i),j.installation_id,'recording/work/'||i,
    printf('af100000-0000-4000-8000-%012x',i),'primary','provider_identity'
FROM artwork_work_indices CROSS JOIN artwork_work_context template JOIN artwork_fetches j ON j.id=template.fetch_id;

INSERT INTO metadata_observations(id,recording_id,release_id,provider,origin,observed_at,title,artist,duration_ms,
    plugin_installation_id,provider_recording_id,provider_metadata,provider_digest,provider_binding_id)
SELECT printf('af300000-0000-4000-8000-%012x',i),printf('af100000-0000-4000-8000-%012x',i),o.release_id,
    o.provider,o.origin,123456,o.title,o.artist,o.duration_ms,o.plugin_installation_id,'recording/work/'||i,
    json_set(o.provider_metadata,'$.id','recording/work/'||i),'owned-artwork-work-'||i,printf('af200000-0000-4000-8000-%012x',i)
FROM artwork_work_indices CROSS JOIN artwork_work_context template
JOIN artwork_fetches j ON j.id=template.fetch_id JOIN metadata_observations o ON o.id=j.observation_id;

UPDATE recordings SET metadata_binding_id=printf('af200000-0000-4000-8000-%012x',fixture.i),
    metadata_observation_id=printf('af300000-0000-4000-8000-%012x',fixture.i)
FROM artwork_work_indices fixture WHERE recordings.id=printf('af100000-0000-4000-8000-%012x',fixture.i);

INSERT INTO artwork_fetches(id,user_id,recording_id,session_hash,observation_id,binding_id,binding_revision,
    installation_id,installation_revision,service_id,service_revision,status,attempts,reservation_bytes,created_at,updated_at)
SELECT printf('af400000-0000-4000-8000-%012x',i),j.user_id,printf('af100000-0000-4000-8000-%012x',i),j.session_hash,
    printf('af300000-0000-4000-8000-%012x',i),printf('af200000-0000-4000-8000-%012x',i),1,
    j.installation_id,j.installation_revision,j.service_id,j.service_revision,'ready',1,0,1,2
FROM artwork_work_indices CROSS JOIN artwork_work_context template JOIN artwork_fetches j ON j.id=template.fetch_id;

INSERT INTO recording_artwork(user_id,recording_id,observation_id,fetch_id,content_hash)
SELECT a.user_id,printf('af100000-0000-4000-8000-%012x',i),printf('af300000-0000-4000-8000-%012x',i),
    printf('af400000-0000-4000-8000-%012x',i),a.content_hash
FROM artwork_work_indices CROSS JOIN artwork_work_context template JOIN recording_artwork a ON a.fetch_id=template.fetch_id;
