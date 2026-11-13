INSERT INTO account_artists(user_id,id,library_id,name) VALUES ('@OWNER@','owned-work-earlier','owned-work-library','Earlier artist'),('@OWNER@','owned-work-ada','owned-work-library','Ada');
INSERT INTO account_recording_credits(user_id,recording_id,artist_id) VALUES ('@OWNER@','00000000-0000-4000-8000-000000000081','owned-work-earlier');
INSERT INTO recording_artist_associations(id,user_id,library_id,recording_id,artist_id,source_id,observation_id,role,provider_kind,provider_artist_id,evidence) VALUES ('@RETIRED@','@OWNER@','owned-work-library','00000000-0000-4000-8000-000000000081','owned-work-earlier','owned-work-source','owned-work-observation','recording_artist','builtin_musicbrainz','00000000-0000-4000-8000-000000000021','user_confirmed');
UPDATE account_recording_credits SET artist_id='owned-work-ada' WHERE user_id='@OWNER@' AND recording_id='00000000-0000-4000-8000-000000000081';
INSERT INTO recording_artist_associations(id,user_id,library_id,recording_id,artist_id,source_id,observation_id,role,provider_kind,provider_artist_id,evidence) VALUES ('@ASSOCIATION@','@OWNER@','owned-work-library','00000000-0000-4000-8000-000000000081','owned-work-ada','owned-work-source','owned-work-observation','recording_artist','builtin_musicbrainz','00000000-0000-4000-8000-000000000020','user_confirmed');
INSERT INTO account_artist_identity_groups(id,user_id,provider_kind,provider_artist_id) VALUES ('@GROUP@','@OWNER@','builtin_musicbrainz','00000000-0000-4000-8000-000000000020');
INSERT INTO account_metadata_policies(user_id,revision,enabled,providers) VALUES ('@OWNER@',1,1,'[{"kind":"builtin_musicbrainz"}]');
INSERT INTO recording_artist_identity_choices(user_id,recording_id,revision,association_id,group_id,artist_id,observation_id,selection_order) VALUES ('@OWNER@','00000000-0000-4000-8000-000000000081',3,'@ASSOCIATION@','@GROUP@','owned-work-ada','owned-work-observation',@RESTORE_ORDER@);
INSERT INTO album_edition_choices(user_id,release_id,revision,recording_id,association_id,provider_kind,release_group_id,edition_id) VALUES ('@OWNER@','00000000-0000-4000-8000-000000000082',1,'00000000-0000-4000-8000-000000000081','@ASSOCIATION@','builtin_musicbrainz','00000000-0000-4000-8000-000000000050','00000000-0000-4000-8000-000000000051');
INSERT INTO recording_artist_association_changes(user_id,recording_id,revision,association_id,linked) VALUES
    ('@OWNER@','00000000-0000-4000-8000-000000000081',1,'@RETIRED@',0),
    ('@OWNER@','00000000-0000-4000-8000-000000000081',2,'@ASSOCIATION@',1),
    ('@OWNER@','00000000-0000-4000-8000-000000000081',3,'@ASSOCIATION@',0),
    ('@OWNER@','00000000-0000-4000-8000-000000000081',4,'@ASSOCIATION@',1);
INSERT INTO artist_identity_changes(id,user_id,group_id,revision,kind,receipt_order) VALUES
    ('@SPLIT@','@OWNER@','@GROUP@',1,'split',@SPLIT_ORDER@),
    ('@RESTORE@','@OWNER@','@GROUP@',2,'undo',@RESTORE_ORDER@);
INSERT INTO artist_identity_change_items(change_id,recording_id,from_revision,from_group_id,from_association_id,from_artist_id,from_observation_id,from_order,to_revision,to_group_id,to_association_id,to_observation_id,to_order) VALUES
    ('@SPLIT@','00000000-0000-4000-8000-000000000081',1,'@GROUP@','@ASSOCIATION@','owned-work-ada','owned-work-observation',@CHOSEN_ORDER@,2,NULL,NULL,NULL,NULL),
    ('@RESTORE@','00000000-0000-4000-8000-000000000081',2,NULL,NULL,'owned-work-ada',NULL,NULL,3,'@GROUP@','@ASSOCIATION@','owned-work-observation',@RESTORE_ORDER@);
INSERT INTO artist_association_change_retirements(user_id,association_id,reason) VALUES ('@OWNER@','@RETIRED@','credit_changed');
INSERT INTO shared_artist_corrections(seq,id,promoted_by,source_kind,source_association_id,change_floor) VALUES (@SEQUENCE@,'@CORRECTION@','@OWNER@','unlink','@ASSOCIATION@',@CHANGE_FLOOR@);
INSERT INTO shared_artist_correction_subjects(correction_seq,recording_id,observation_id,provider_artist_id,raw_artist) VALUES (@SEQUENCE@,'00000000-0000-4000-8000-000000000081','owned-work-observation','00000000-0000-4000-8000-000000000020','Ada');
INSERT INTO shared_artist_correction_states(correction_seq,revision,applied,changed_by) VALUES (@SEQUENCE@,1,1,'@OWNER@'),(@SEQUENCE@,2,0,'@OWNER@');
INSERT INTO artist_association_matches(association_id,evidence_kind,receipt_order) VALUES ('@ASSOCIATION@','file_tag_artist_id',@CHOSEN_ORDER@);
INSERT INTO artist_association_provider_matches(association_id,evidence_kind,receipt_order) VALUES ('@ASSOCIATION@','isrc',@CHOSEN_ORDER@);
INSERT INTO artist_association_name_matches(association_id,evidence_kind,receipt_order) VALUES ('@ASSOCIATION@','name_discography',@CHOSEN_ORDER@);
INSERT INTO artist_association_credit_matches(association_id,evidence_kind,source_tracks,receipt_order) VALUES ('@ASSOCIATION@','provider_url',2,@CHOSEN_ORDER@);
INSERT INTO artist_association_coverage_matches(association_id,matched_tracks,probed_tracks,receipt_order) VALUES ('@ASSOCIATION@',2,3,@CHOSEN_ORDER@);
INSERT INTO artist_association_candidate_confirmations(association_id,evidence_kind,receipt_order) VALUES ('@ASSOCIATION@','user_confirmed_candidate',@CHOSEN_ORDER@);
INSERT INTO artist_association_recording_id_matches(association_id,receipt_order) VALUES ('@ASSOCIATION@',@CHOSEN_ORDER@);
