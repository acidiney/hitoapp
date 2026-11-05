CREATE TABLE plugin_connections (
    installation_id TEXT PRIMARY KEY REFERENCES plugin_installations(id) ON DELETE CASCADE,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    service_id TEXT NOT NULL REFERENCES plugin_services(id) ON DELETE CASCADE,
    declaration_digest BLOB NOT NULL CHECK(typeof(declaration_digest) = 'blob' AND length(declaration_digest) = 32),
    status TEXT NOT NULL CHECK(status IN ('connected','needs_reauthorization')),
    tokens BLOB CHECK(tokens IS NULL OR (typeof(tokens) = 'blob' AND length(tokens) BETWEEN 74 AND 8800)),
    scopes TEXT NOT NULL CHECK(length(scopes) <= 4096),
    expires_at INTEGER,
    refresh_failures INTEGER NOT NULL DEFAULT 0 CHECK(refresh_failures >= 0),
    refresh_failing_since INTEGER,
    connected_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK((status = 'connected') = (tokens IS NOT NULL))
);
CREATE INDEX account_plugin_connections ON plugin_connections(user_id);
CREATE INDEX service_plugin_connections ON plugin_connections(service_id);
CREATE INDEX plugin_connection_fingerprints ON plugin_connections(substr(tokens, 1, 33)) WHERE tokens IS NOT NULL;
