CREATE TEMP TABLE plugin_authorization_attempt_backup AS
    SELECT rowid AS retained_rowid,id,user_id,session_hash,installation_id,service_id,installation_revision,service_revision,redirect_uri,state_hash,pending_secret,created_at,expires_at,status,delivery
    FROM plugin_authorization_attempts;

DROP TABLE plugin_authorization_attempts;

CREATE TABLE plugin_authorization_attempts (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    session_hash TEXT NOT NULL REFERENCES sessions(token_hash) ON DELETE CASCADE,
    installation_id TEXT NOT NULL REFERENCES plugin_installations(id) ON DELETE CASCADE,
    service_id TEXT NOT NULL REFERENCES plugin_services(id) ON DELETE CASCADE,
    installation_revision INTEGER NOT NULL CHECK(installation_revision > 0),
    service_revision INTEGER NOT NULL CHECK(service_revision > 0),
    redirect_uri TEXT NOT NULL CHECK(length(redirect_uri) BETWEEN 1 AND 2048),
    state_hash BLOB NOT NULL UNIQUE CHECK(typeof(state_hash) = 'blob' AND length(state_hash) = 32),
    pending_secret BLOB NOT NULL CHECK(typeof(pending_secret) = 'blob' AND length(pending_secret) BETWEEN 74 AND 1097),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    expires_at INTEGER NOT NULL CHECK(expires_at > created_at AND expires_at <= created_at + 600),
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','returned','exchanging','completed','failed')),
    delivery TEXT NOT NULL DEFAULT 'session' CHECK(delivery IN ('session','loopback','native')),
    completion_hash BLOB CHECK(completion_hash IS NULL OR (typeof(completion_hash) = 'blob' AND length(completion_hash) = 32)),
    return_hash BLOB CHECK(return_hash IS NULL OR (typeof(return_hash) = 'blob' AND length(return_hash) = 32)),
    returned_code BLOB CHECK(returned_code IS NULL OR (typeof(returned_code) = 'blob' AND length(returned_code) BETWEEN 74 AND 4169)),
    CHECK((delivery = 'native') = (completion_hash IS NOT NULL)),
    CHECK((status = 'returned') = (returned_code IS NOT NULL)),
    CHECK((returned_code IS NULL) = (return_hash IS NULL)),
    CHECK(status <> 'returned' OR delivery = 'native')
);

INSERT INTO plugin_authorization_attempts(rowid,id,user_id,session_hash,installation_id,service_id,installation_revision,service_revision,redirect_uri,state_hash,pending_secret,created_at,expires_at,status,delivery)
    SELECT retained_rowid,id,user_id,session_hash,installation_id,service_id,installation_revision,service_revision,redirect_uri,state_hash,pending_secret,created_at,expires_at,status,delivery
    FROM plugin_authorization_attempt_backup;

DROP TABLE plugin_authorization_attempt_backup;

CREATE INDEX account_plugin_authorization_attempts ON plugin_authorization_attempts(user_id, expires_at);
CREATE INDEX session_plugin_authorization_attempts ON plugin_authorization_attempts(session_hash);
CREATE INDEX expired_plugin_authorization_attempts ON plugin_authorization_attempts(expires_at, id);
CREATE INDEX plugin_authorization_fingerprints ON plugin_authorization_attempts(substr(pending_secret, 1, 33));
