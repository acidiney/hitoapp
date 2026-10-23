CREATE TABLE plugin_services (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0, 1)),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision > 0)
);
CREATE UNIQUE INDEX active_plugin_endpoints ON plugin_services(base_url) WHERE active = 1;

CREATE TABLE plugin_installations (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    service_id TEXT NOT NULL REFERENCES plugin_services(id),
    manifest TEXT NOT NULL,
    credential BLOB,
    active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0, 1)),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision > 0),
    checked_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE UNIQUE INDEX active_plugin_installations ON plugin_installations(user_id, service_id) WHERE active = 1;
CREATE INDEX account_plugin_installations ON plugin_installations(user_id, active, checked_at DESC, id);
CREATE INDEX service_plugin_installations ON plugin_installations(service_id, active);
CREATE INDEX plugin_credential_fingerprints
    ON plugin_installations(substr(credential, 1, 33))
    WHERE credential IS NOT NULL;
