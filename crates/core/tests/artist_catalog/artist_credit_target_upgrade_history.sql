INSERT INTO recording_artist_identity_choices(user_id,recording_id,revision,association_id,group_id,artist_id,observation_id,selection_order) VALUES ('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000001',3,'00000000-0000-4000-8000-000000000002','00000000-0000-4000-8000-000000000003','owned-upgrade-ada','owned-upgrade-observation',703);
INSERT INTO album_edition_choices(user_id,release_id,revision,recording_id,association_id,provider_kind,release_group_id,edition_id) VALUES ('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000005',1,'00000000-0000-4000-8000-000000000001','00000000-0000-4000-8000-000000000002','builtin_musicbrainz','00000000-0000-4000-8000-000000000050','00000000-0000-4000-8000-000000000051');
INSERT INTO recording_artist_association_changes(user_id,recording_id,revision,association_id,linked) VALUES
    ('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000001',1,'00000000-0000-4000-8000-000000000004',0),
    ('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000001',2,'00000000-0000-4000-8000-000000000002',1),
    ('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000001',3,'00000000-0000-4000-8000-000000000002',0),
    ('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000001',4,'00000000-0000-4000-8000-000000000002',1),
    ('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000001',5,'00000000-0000-4000-8000-000000000062',1);
INSERT INTO artist_identity_changes(id,user_id,group_id,revision,kind,receipt_order) VALUES
    ('00000000-0000-4000-8000-000000000070','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000003',1,'split',702),
    ('00000000-0000-4000-8000-000000000071','00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000003',2,'undo',703);
INSERT INTO artist_identity_change_items(change_id,recording_id,from_revision,from_group_id,from_association_id,from_artist_id,from_observation_id,from_order,to_revision,to_group_id,to_association_id,to_observation_id,to_order) VALUES
    ('00000000-0000-4000-8000-000000000070','00000000-0000-4000-8000-000000000001',1,'00000000-0000-4000-8000-000000000003','00000000-0000-4000-8000-000000000002','owned-upgrade-ada','owned-upgrade-observation',701,2,NULL,NULL,NULL,NULL),
    ('00000000-0000-4000-8000-000000000071','00000000-0000-4000-8000-000000000001',2,NULL,NULL,'owned-upgrade-ada',NULL,NULL,3,'00000000-0000-4000-8000-000000000003','00000000-0000-4000-8000-000000000002','owned-upgrade-observation',703);
INSERT INTO artist_association_change_retirements(user_id,association_id,reason) VALUES ('00000000-0000-4000-8000-000000000010','00000000-0000-4000-8000-000000000004','credit_changed');
INSERT INTO shared_artist_corrections(seq,id,promoted_by,source_kind,source_association_id,change_floor) VALUES (1,'00000000-0000-4000-8000-000000000040','00000000-0000-4000-8000-000000000010','unlink','00000000-0000-4000-8000-000000000002',3);
INSERT INTO shared_artist_correction_subjects(correction_seq,recording_id,observation_id,provider_artist_id,raw_artist) VALUES (1,'00000000-0000-4000-8000-000000000001','owned-upgrade-observation','00000000-0000-4000-8000-000000000020','Ada');
INSERT INTO shared_artist_correction_states(correction_seq,revision,applied,changed_by) VALUES (1,1,1,'00000000-0000-4000-8000-000000000010'),(1,2,0,'00000000-0000-4000-8000-000000000010');
INSERT INTO artist_association_matches(association_id,evidence_kind,receipt_order) VALUES ('00000000-0000-4000-8000-000000000002','file_tag_artist_id',701);
INSERT INTO artist_association_provider_matches(association_id,evidence_kind,receipt_order) VALUES ('00000000-0000-4000-8000-000000000002','isrc',701);
INSERT INTO artist_association_name_matches(association_id,evidence_kind,receipt_order) VALUES ('00000000-0000-4000-8000-000000000002','name_discography',701);
INSERT INTO artist_association_credit_matches(association_id,evidence_kind,source_tracks,receipt_order) VALUES ('00000000-0000-4000-8000-000000000002','provider_url',2,701);
INSERT INTO artist_association_coverage_matches(association_id,matched_tracks,probed_tracks,receipt_order) VALUES ('00000000-0000-4000-8000-000000000002',2,3,701);
INSERT INTO artist_association_candidate_confirmations(association_id,evidence_kind,receipt_order) VALUES ('00000000-0000-4000-8000-000000000002','user_confirmed_candidate',701);
INSERT INTO artist_association_recording_id_matches(association_id,receipt_order) VALUES ('00000000-0000-4000-8000-000000000002',701);
