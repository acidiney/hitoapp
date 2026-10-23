UPDATE plugin_installations SET manifest=json_insert(manifest,'$.capabilities[#]','metadata') WHERE id='migration-installation';
INSERT INTO source_resolutions(rowid,id,user_id,recording_id,target_digest,choice_revision,outcome,created_at,completed_at) VALUES
    (2099,'musical-resolution','migration-owner','evidence-recording',
        '3333333333333333333333333333333333333333333333333333333333333333',7,'ready',1220,1230);
INSERT INTO source_resolution_receipts(rowid,user_id,request_id,recording_id,resolution_id,created_at) VALUES
    (2111,'migration-owner','owned-musical-request','evidence-recording','musical-resolution',1221);
INSERT INTO automatic_source_decisions(rowid,resolution_id,binding_id,policy_version,target_digest,candidate_digest,installation_revision,service_revision,created_at,musical_metadata) VALUES
    (2129,'musical-resolution','evidence-automatic','hito.musical-source.metadata.v1',
        '3333333333333333333333333333333333333333333333333333333333333333',
        '4444444444444444444444444444444444444444444444444444444444444444',1,1,1230,
        '{"recording":{"id":"owned-evidence-automatic","title":"Owned evidence track","artist":"Owned evidence artist","duration_ms":12000,"release":null,"disc_number":null,"track_number":null,"revision":null},"musical_title":"Owned evidence track","album_title":"Owned evidence album","source_ids":["owned-evidence-source"]}');
