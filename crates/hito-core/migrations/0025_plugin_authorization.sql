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
    expires_at INTEGER NOT NULL CHECK(expires_at > created_at AND expires_at <= created_at + 600)
);
CREATE INDEX account_plugin_authorization_attempts ON plugin_authorization_attempts(user_id, expires_at);
CREATE INDEX session_plugin_authorization_attempts ON plugin_authorization_attempts(session_hash);
CREATE INDEX expired_plugin_authorization_attempts ON plugin_authorization_attempts(expires_at, id);
CREATE INDEX plugin_authorization_fingerprints ON plugin_authorization_attempts(substr(pending_secret, 1, 33));
