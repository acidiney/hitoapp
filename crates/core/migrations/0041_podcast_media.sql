CREATE TABLE podcast_sources (
    episode_id TEXT PRIMARY KEY NOT NULL REFERENCES podcast_episodes(id),
    source_id TEXT NOT NULL UNIQUE REFERENCES sources(id)
);

CREATE TRIGGER podcast_source_owner BEFORE INSERT ON sources
WHEN new.provider='podcast' AND NOT EXISTS(
    SELECT 1 FROM podcast_episodes e
    JOIN podcast_subscriptions p ON p.id=e.subscription_id AND p.user_id=e.user_id
    JOIN recordings r ON r.id=e.recording_id AND r.library_id=p.library_id
    JOIN libraries l ON l.id=p.library_id AND l.owner_id=e.user_id
    WHERE e.id=new.provider_id AND e.recording_id=new.recording_id AND new.device_id IS NULL
) BEGIN
    SELECT RAISE(ABORT, 'Podcast sources require their owned episode recording');
END;
CREATE TRIGGER podcast_source_identity BEFORE UPDATE ON sources
WHEN (old.provider='podcast' OR new.provider='podcast')
    AND (old.id IS NOT new.id OR old.recording_id IS NOT new.recording_id
        OR old.provider IS NOT new.provider OR old.provider_id IS NOT new.provider_id
        OR old.device_id IS NOT new.device_id) BEGIN
    SELECT RAISE(ABORT, 'Podcast source identity is immutable');
END;
CREATE TRIGGER podcast_source_retained BEFORE DELETE ON sources
WHEN old.provider='podcast' BEGIN
    SELECT RAISE(ABORT, 'Podcast sources must be retained');
END;

CREATE TRIGGER podcast_source_mapping_owner BEFORE INSERT ON podcast_sources
WHEN NOT EXISTS(
    SELECT 1 FROM podcast_episodes e
    JOIN sources s ON s.recording_id=e.recording_id AND s.provider='podcast'
        AND s.provider_id=e.id AND s.device_id IS NULL
    WHERE e.id=new.episode_id AND s.id=new.source_id
) BEGIN
    SELECT RAISE(ABORT, 'Podcast source mapping must match its episode recording');
END;
CREATE TRIGGER podcast_source_mapping_identity BEFORE UPDATE ON podcast_sources
WHEN old.episode_id IS NOT new.episode_id OR old.source_id IS NOT new.source_id BEGIN
    SELECT RAISE(ABORT, 'Podcast source mapping is immutable');
END;
CREATE TRIGGER podcast_source_mapping_retained BEFORE DELETE ON podcast_sources BEGIN
    SELECT RAISE(ABORT, 'Podcast source mappings must be retained');
END;

CREATE TEMP TABLE hito_podcast_media_backfill (
    episode_id TEXT PRIMARY KEY NOT NULL,
    source_id TEXT NOT NULL UNIQUE
);
INSERT INTO hito_podcast_media_backfill(episode_id,source_id)
SELECT id,lower(hex(randomblob(4)) || '-' || hex(randomblob(2)) || '-4'
    || substr(hex(randomblob(2)),2,3) || '-'
    || substr('89ab',(random() & 3)+1,1) || substr(hex(randomblob(2)),2,3)
    || '-' || hex(randomblob(6)))
FROM podcast_episodes;
INSERT INTO sources(id,recording_id,provider,provider_id,device_id,ready)
SELECT generated.source_id,e.recording_id,'podcast',e.id,NULL,1
FROM hito_podcast_media_backfill generated JOIN podcast_episodes e ON e.id=generated.episode_id;
INSERT INTO podcast_sources(episode_id,source_id)
SELECT episode_id,source_id FROM hito_podcast_media_backfill;
DROP TABLE hito_podcast_media_backfill;

CREATE VIEW current_podcast_sources AS
SELECT s.id AS source_id,e.id AS episode_id,e.recording_id,e.user_id,e.subscription_id,
    p.generation AS subscription_generation,e.enclosure_revision,e.enclosure_url,e.declared_mime,e.byte_length
FROM current_podcast_authority authority
JOIN podcast_subscriptions p ON p.id=authority.id
JOIN podcast_episodes e ON e.subscription_id=p.id AND e.user_id=p.user_id
JOIN recordings r ON r.id=e.recording_id AND r.library_id=p.library_id
JOIN podcast_sources mapping ON mapping.episode_id=e.id
JOIN sources s ON s.id=mapping.source_id AND s.recording_id=e.recording_id
    AND s.provider='podcast' AND s.provider_id=e.id AND s.device_id IS NULL AND s.ready=1;
