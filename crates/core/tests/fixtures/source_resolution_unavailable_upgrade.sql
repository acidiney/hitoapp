INSERT INTO source_resolutions(rowid,id,user_id,recording_id,target_digest,choice_revision,outcome,lease_token,lease_until,created_at,completed_at) VALUES
    (3101,'unavailable-no-match','migration-owner','evidence-recording','1111111111111111111111111111111111111111111111111111111111111111',7,'no_suitable_source',NULL,NULL,1300,1301),
    (3109,'unavailable-protected','migration-owner','evidence-recording','1111111111111111111111111111111111111111111111111111111111111111',7,'protected_choice',NULL,NULL,1310,1311),
    (3119,'unavailable-cancelled','migration-owner','evidence-recording','1111111111111111111111111111111111111111111111111111111111111111',7,'cancelled',NULL,NULL,1320,1321),
    (3121,'unavailable-pending','migration-owner','evidence-recording','1111111111111111111111111111111111111111111111111111111111111111',7,NULL,'owned-pending-lease',9999999999,1330,NULL),
    (3137,'unavailable-musical-ready','migration-owner','evidence-recording','1111111111111111111111111111111111111111111111111111111111111111',7,'ready',NULL,NULL,1340,1341);
INSERT INTO source_resolution_receipts(rowid,user_id,request_id,recording_id,resolution_id,created_at) VALUES
    (3203,'migration-owner','owned-no-match-request','evidence-recording','unavailable-no-match',1300),
    (3209,'migration-owner','owned-protected-request','evidence-recording','unavailable-protected',1310),
    (3217,'migration-owner','owned-cancelled-request','evidence-recording','unavailable-cancelled',1320),
    (3221,'migration-owner','owned-pending-request','evidence-recording','unavailable-pending',1330),
    (3229,'migration-owner','owned-musical-ready-request','evidence-recording','unavailable-musical-ready',1340);
INSERT INTO automatic_source_decisions(rowid,resolution_id,binding_id,policy_version,target_digest,candidate_digest,installation_revision,service_revision,created_at,musical_metadata) VALUES
    (3301,'unavailable-musical-ready','evidence-automatic','hito.musical-source.metadata.v1',
        '1111111111111111111111111111111111111111111111111111111111111111',
        '2222222222222222222222222222222222222222222222222222222222222222',1,1,1341,
        '{"recording":{"id":"owned-evidence-automatic","title":"Owned evidence track","artist":"Owned evidence artist","duration_ms":12000},"musical_title":"Owned evidence track","source_ids":["owned-evidence-source"]}');
