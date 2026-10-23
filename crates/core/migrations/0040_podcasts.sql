CREATE TABLE podcast_subscriptions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    library_id TEXT NOT NULL REFERENCES libraries(id),
    access_grant_id TEXT NOT NULL,
    feed_url TEXT NOT NULL,
    fetch_url TEXT NOT NULL,
    generation INTEGER NOT NULL DEFAULT 1 CHECK(generation > 0),
    active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0,1)),
    title TEXT,
    author TEXT,
    description TEXT,
    episode_count INTEGER NOT NULL DEFAULT 0 CHECK(episode_count >= 0),
    content_revision INTEGER NOT NULL DEFAULT 0 CHECK(content_revision >= 0),
    last_refreshed_at INTEGER,
    next_refresh_at INTEGER NOT NULL DEFAULT (unixepoch()),
    etag TEXT,
    last_modified TEXT,
    error TEXT CHECK(error IS NULL OR error IN ('feed_unavailable','invalid_feed','lease_expired','authority_changed')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(user_id,feed_url),
    UNIQUE(id,user_id)
);
CREATE INDEX podcast_subscription_due ON podcast_subscriptions(active,next_refresh_at,id);
CREATE TRIGGER podcast_subscription_owner BEFORE INSERT ON podcast_subscriptions
WHEN NOT EXISTS(SELECT 1 FROM libraries l JOIN library_access_grants g ON g.library_id=l.id
    WHERE l.id=new.library_id AND l.owner_id=new.user_id AND g.user_id=new.user_id AND g.id=new.access_grant_id) BEGIN
    SELECT RAISE(ABORT, 'Podcast subscriptions require their account library');
END;
CREATE TRIGGER podcast_subscription_identity BEFORE UPDATE ON podcast_subscriptions
WHEN old.id IS NOT new.id OR old.user_id IS NOT new.user_id OR old.library_id IS NOT new.library_id
    OR old.access_grant_id IS NOT new.access_grant_id OR old.feed_url IS NOT new.feed_url BEGIN
    SELECT RAISE(ABORT, 'Podcast subscription identity is immutable');
END;

CREATE TABLE podcast_episodes (
    id TEXT PRIMARY KEY,
    subscription_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    recording_id TEXT NOT NULL UNIQUE REFERENCES recordings(id),
    identity_kind TEXT NOT NULL CHECK(identity_kind IN ('guid','enclosure')),
    identity_value TEXT NOT NULL,
    enclosure_url TEXT NOT NULL,
    enclosure_revision INTEGER NOT NULL DEFAULT 1 CHECK(enclosure_revision > 0),
    declared_mime TEXT,
    byte_length INTEGER CHECK(byte_length IS NULL OR byte_length >= 0),
    description TEXT,
    published_at INTEGER,
    sort_key INTEGER GENERATED ALWAYS AS (coalesce(published_at,-9223372036854775808)) STORED,
    first_seen_at INTEGER NOT NULL DEFAULT (unixepoch()),
    last_seen_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE(subscription_id,identity_kind,identity_value),
    FOREIGN KEY(subscription_id,user_id) REFERENCES podcast_subscriptions(id,user_id)
);
CREATE INDEX podcast_episode_order ON podcast_episodes(subscription_id,sort_key DESC,id DESC);
CREATE TRIGGER podcast_episode_owner BEFORE INSERT ON podcast_episodes
WHEN NOT EXISTS(SELECT 1 FROM podcast_subscriptions p JOIN recordings r ON r.library_id=p.library_id
    WHERE p.id=new.subscription_id AND p.user_id=new.user_id AND r.id=new.recording_id) BEGIN
    SELECT RAISE(ABORT, 'Podcast episodes require their account library');
END;
CREATE TRIGGER podcast_episode_identity BEFORE UPDATE ON podcast_episodes
WHEN old.id IS NOT new.id OR old.subscription_id IS NOT new.subscription_id OR old.user_id IS NOT new.user_id
    OR old.recording_id IS NOT new.recording_id OR old.identity_kind IS NOT new.identity_kind
    OR old.identity_value IS NOT new.identity_value BEGIN
    SELECT RAISE(ABORT, 'Podcast episode identity is immutable');
END;
CREATE TRIGGER podcast_episode_count AFTER INSERT ON podcast_episodes BEGIN
    UPDATE podcast_subscriptions SET episode_count=episode_count+1 WHERE id=new.subscription_id;
END;
CREATE TRIGGER podcast_episode_retained BEFORE DELETE ON podcast_episodes BEGIN
    SELECT RAISE(ABORT, 'Podcast episode identity must be retained');
END;
CREATE TRIGGER podcast_recording_library BEFORE UPDATE OF library_id ON recordings
WHEN old.library_id IS NOT new.library_id AND EXISTS(SELECT 1 FROM podcast_episodes e WHERE e.recording_id=old.id) BEGIN
    SELECT RAISE(ABORT, 'Podcast recording ownership is immutable');
END;

CREATE TABLE podcast_refresh_jobs (
    id TEXT PRIMARY KEY,
    subscription_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK(generation > 0),
    status TEXT NOT NULL CHECK(status IN ('queued','running','complete','failed','cancelled')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts BETWEEN 0 AND 3),
    next_attempt_at INTEGER NOT NULL DEFAULT (unixepoch()),
    lease_token TEXT,
    lease_until INTEGER,
    error TEXT CHECK(error IS NULL OR error IN ('feed_unavailable','invalid_feed','lease_expired','authority_changed')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK((lease_token IS NULL) = (lease_until IS NULL)),
    CHECK((status = 'running') = (lease_token IS NOT NULL)),
    FOREIGN KEY(subscription_id,user_id) REFERENCES podcast_subscriptions(id,user_id)
);
CREATE UNIQUE INDEX podcast_refresh_pending ON podcast_refresh_jobs(subscription_id) WHERE status IN ('queued','running');
CREATE INDEX podcast_refresh_queue ON podcast_refresh_jobs(status,next_attempt_at,created_at,id);
CREATE INDEX podcast_refresh_account ON podcast_refresh_jobs(user_id,status);
CREATE INDEX podcast_refresh_expired ON podcast_refresh_jobs(lease_until) WHERE status='running';
CREATE INDEX podcast_refresh_history ON podcast_refresh_jobs(subscription_id,created_at ASC);

CREATE TABLE podcast_request_account_counts (
    user_id TEXT PRIMARY KEY REFERENCES users(id),
    receipt_count INTEGER NOT NULL CHECK(receipt_count BETWEEN 0 AND 4096)
) WITHOUT ROWID;
CREATE TRIGGER podcast_request_account_count_retained BEFORE DELETE ON podcast_request_account_counts BEGIN
    SELECT RAISE(ABORT, 'Podcast request counts must be retained');
END;
CREATE TRIGGER podcast_request_account_count_identity BEFORE UPDATE OF user_id ON podcast_request_account_counts
WHEN old.user_id IS NOT new.user_id BEGIN
    SELECT RAISE(ABORT, 'Podcast request count ownership is immutable');
END;

CREATE TABLE podcast_request_total (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    receipt_count INTEGER NOT NULL CHECK(receipt_count BETWEEN 0 AND 65536)
);
INSERT INTO podcast_request_total(singleton,receipt_count) VALUES(1,0);
CREATE TRIGGER podcast_request_total_retained BEFORE DELETE ON podcast_request_total BEGIN
    SELECT RAISE(ABORT, 'Podcast request totals must be retained');
END;

CREATE TABLE podcast_subscription_receipts (
    user_id TEXT NOT NULL REFERENCES users(id),
    request_id TEXT NOT NULL,
    request_digest TEXT NOT NULL,
    subscription_id TEXT NOT NULL,
    job_id TEXT NOT NULL REFERENCES podcast_refresh_jobs(id),
    PRIMARY KEY(user_id,request_id),
    FOREIGN KEY(subscription_id,user_id) REFERENCES podcast_subscriptions(id,user_id)
);
CREATE TRIGGER podcast_subscription_receipt_retained BEFORE DELETE ON podcast_subscription_receipts BEGIN
    SELECT RAISE(ABORT, 'Podcast subscription receipts must be retained');
END;
CREATE TRIGGER podcast_subscription_receipt_immutable BEFORE UPDATE ON podcast_subscription_receipts BEGIN
    SELECT RAISE(ABORT, 'Podcast subscription receipts are immutable');
END;
CREATE TRIGGER podcast_subscription_receipt_count AFTER INSERT ON podcast_subscription_receipts BEGIN
    INSERT INTO podcast_request_account_counts(user_id,receipt_count) VALUES(new.user_id,1)
    ON CONFLICT(user_id) DO UPDATE SET receipt_count=receipt_count+1;
    UPDATE podcast_request_total SET receipt_count=receipt_count+1 WHERE singleton=1;
END;

CREATE TABLE podcast_refresh_receipts (
    user_id TEXT NOT NULL REFERENCES users(id),
    request_id TEXT NOT NULL,
    subscription_id TEXT NOT NULL,
    job_id TEXT NOT NULL REFERENCES podcast_refresh_jobs(id),
    PRIMARY KEY(user_id,request_id),
    FOREIGN KEY(subscription_id,user_id) REFERENCES podcast_subscriptions(id,user_id)
);
CREATE TRIGGER podcast_refresh_receipt_retained BEFORE DELETE ON podcast_refresh_receipts BEGIN
    SELECT RAISE(ABORT, 'Podcast refresh receipts must be retained');
END;
CREATE TRIGGER podcast_refresh_receipt_immutable BEFORE UPDATE ON podcast_refresh_receipts BEGIN
    SELECT RAISE(ABORT, 'Podcast refresh receipts are immutable');
END;
CREATE TRIGGER podcast_refresh_receipt_count AFTER INSERT ON podcast_refresh_receipts BEGIN
    INSERT INTO podcast_request_account_counts(user_id,receipt_count) VALUES(new.user_id,1)
    ON CONFLICT(user_id) DO UPDATE SET receipt_count=receipt_count+1;
    UPDATE podcast_request_total SET receipt_count=receipt_count+1 WHERE singleton=1;
END;

CREATE VIEW current_podcast_authority AS
SELECT p.id FROM podcast_subscriptions p
JOIN libraries l ON l.id=p.library_id AND l.owner_id=p.user_id
JOIN library_access_grants g ON g.library_id=p.library_id AND g.user_id=p.user_id AND g.id=p.access_grant_id
WHERE p.active=1;
