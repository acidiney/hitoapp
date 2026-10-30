CREATE TABLE erased_download_files (
    file_id TEXT PRIMARY KEY,
    attempt_id TEXT NOT NULL,
    host_device TEXT NOT NULL
);
CREATE INDEX erased_download_files_host ON erased_download_files(host_device,file_id);

DROP TRIGGER download_receipt_retained;
CREATE TRIGGER download_receipt_retained BEFORE DELETE ON download_receipts
WHEN NOT EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.account_id) BEGIN
    SELECT RAISE(ABORT, 'Download receipts must be retained');
END;

DROP TRIGGER download_retention_retained;
CREATE TRIGGER download_retention_retained BEFORE DELETE ON download_retentions
WHEN NOT EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.account_id) BEGIN
    SELECT RAISE(ABORT, 'Download retention identities must be retained');
END;

DROP TRIGGER download_playlist_receipt_retained;
CREATE TRIGGER download_playlist_receipt_retained BEFORE DELETE ON download_playlist_receipts
WHEN NOT EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.account_id) BEGIN
    SELECT RAISE(ABORT, 'Playlist download receipts must be retained');
END;

DROP TRIGGER download_terminal_failure_retained;
CREATE TRIGGER download_terminal_failure_retained BEFORE DELETE ON download_terminal_failures
WHEN NOT EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=(SELECT i.account_id FROM download_intents i WHERE i.id=old.intent_id)) BEGIN
    SELECT RAISE(ABORT, 'Download terminal failures must be retained');
END;

DROP TRIGGER provider_item_retained;
CREATE TRIGGER provider_item_retained BEFORE DELETE ON provider_items
WHEN NOT EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=(SELECT p.user_id FROM plugin_installations p WHERE p.id=old.installation_id)) BEGIN
    SELECT RAISE(ABORT, 'Provider binding history must be retained');
END;

DROP TRIGGER source_resolution_receipt_retained;
CREATE TRIGGER source_resolution_receipt_retained BEFORE DELETE ON source_resolution_receipts
WHEN NOT EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.user_id) BEGIN
    SELECT RAISE(ABORT, 'Source resolution receipts must be retained');
END;

DROP TRIGGER automatic_source_decision_retained;
CREATE TRIGGER automatic_source_decision_retained BEFORE DELETE ON automatic_source_decisions
WHEN NOT EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=(SELECT r.user_id FROM source_resolutions r WHERE r.id=old.resolution_id)) BEGIN
    SELECT RAISE(ABORT, 'Automatic source decisions must be retained');
END;

DROP TRIGGER source_choice_receipt_retained;
CREATE TRIGGER source_choice_receipt_retained BEFORE DELETE ON source_choice_receipts
WHEN NOT EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.user_id) BEGIN
    SELECT RAISE(ABORT, 'Source choice receipts must be retained');
END;

DROP TRIGGER source_choice_reset_receipt_retained;
CREATE TRIGGER source_choice_reset_receipt_retained BEFORE DELETE ON source_choice_reset_receipts
WHEN NOT EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.user_id) BEGIN
    SELECT RAISE(ABORT, 'Source choice reset receipts must be retained');
END;

DROP TRIGGER podcast_episode_retained;
CREATE TRIGGER podcast_episode_retained BEFORE DELETE ON podcast_episodes
WHEN NOT EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.user_id) BEGIN
    SELECT RAISE(ABORT, 'Podcast episode identity must be retained');
END;

DROP TRIGGER podcast_request_account_count_retained;
CREATE TRIGGER podcast_request_account_count_retained BEFORE DELETE ON podcast_request_account_counts
WHEN NOT EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.user_id) BEGIN
    SELECT RAISE(ABORT, 'Podcast request counts must be retained');
END;

DROP TRIGGER podcast_subscription_receipt_retained;
CREATE TRIGGER podcast_subscription_receipt_retained BEFORE DELETE ON podcast_subscription_receipts
WHEN NOT EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.user_id) BEGIN
    SELECT RAISE(ABORT, 'Podcast subscription receipts must be retained');
END;

DROP TRIGGER podcast_refresh_receipt_retained;
CREATE TRIGGER podcast_refresh_receipt_retained BEFORE DELETE ON podcast_refresh_receipts
WHEN NOT EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.user_id) BEGIN
    SELECT RAISE(ABORT, 'Podcast refresh receipts must be retained');
END;

DROP TRIGGER podcast_unsubscribe_receipt_retained;
CREATE TRIGGER podcast_unsubscribe_receipt_retained BEFORE DELETE ON podcast_unsubscribe_receipts
WHEN NOT EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.user_id) BEGIN
    SELECT RAISE(ABORT, 'Podcast unsubscribe receipts must be retained');
END;

DROP TRIGGER podcast_source_mapping_retained;
CREATE TRIGGER podcast_source_mapping_retained BEFORE DELETE ON podcast_sources
WHEN NOT EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=(SELECT e.user_id FROM podcast_episodes e WHERE e.id=old.episode_id)) BEGIN
    SELECT RAISE(ABORT, 'Podcast source mappings must be retained');
END;

DROP TRIGGER podcast_source_retained;
CREATE TRIGGER podcast_source_retained BEFORE DELETE ON sources
WHEN old.provider='podcast' AND NOT EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=(SELECT l.owner_id FROM recordings r JOIN libraries l ON l.id=r.library_id WHERE r.id=old.recording_id)) BEGIN
    SELECT RAISE(ABORT, 'Podcast sources must be retained');
END;

DROP TRIGGER playlist_metadata_receipt_retained;
CREATE TRIGGER playlist_metadata_receipt_retained BEFORE DELETE ON playlist_metadata_refresh_receipts
WHEN NOT EXISTS (SELECT 1 FROM correction_erasure_scopes s WHERE s.kind='account' AND s.subject=old.user_id) BEGIN
    SELECT RAISE(ABORT, 'Playlist metadata receipts must be retained');
END;
