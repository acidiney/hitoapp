CREATE TABLE replica_connections (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    endpoint TEXT NOT NULL,
    remote_account_id TEXT NOT NULL,
    remote_user TEXT NOT NULL,
    client_id TEXT NOT NULL,
    generation INTEGER NOT NULL DEFAULT 1 CHECK(generation > 0),
    state TEXT NOT NULL CHECK(state IN ('initializing','ready','disconnected')),
    credential BLOB,
    UNIQUE(user_id,endpoint,remote_account_id)
);
CREATE INDEX replica_credential_fingerprints ON replica_connections(substr(credential,1,33)) WHERE credential IS NOT NULL;
CREATE TABLE replica_activations (
    session_hash TEXT PRIMARY KEY REFERENCES sessions(token_hash) ON DELETE CASCADE,
    connection_id TEXT NOT NULL REFERENCES replica_connections(id),
    activation_id TEXT NOT NULL
);
CREATE TABLE erased_replica_profiles (id TEXT PRIMARY KEY);
CREATE TABLE replica_state (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    binding_id TEXT NOT NULL,
    remote_account_id TEXT NOT NULL REFERENCES users(id),
    client_id TEXT NOT NULL,
    cursor INTEGER NOT NULL DEFAULT 0 CHECK(cursor >= 0)
);
CREATE TABLE replica_base_playlists (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    deleted INTEGER NOT NULL CHECK(deleted IN (0,1))
);
CREATE TABLE replica_outbox (
    ordinal INTEGER PRIMARY KEY AUTOINCREMENT,
    operation_id TEXT NOT NULL UNIQUE,
    playlist_id TEXT NOT NULL,
    payload TEXT NOT NULL,
    acknowledged INTEGER NOT NULL DEFAULT 0 CHECK(acknowledged IN (0,1)),
    echoed INTEGER NOT NULL DEFAULT 0 CHECK(echoed IN (0,1))
);
CREATE TABLE replica_received_events (
    sequence INTEGER PRIMARY KEY CHECK(sequence > 0),
    operation_id TEXT NOT NULL UNIQUE,
    client_id TEXT NOT NULL,
    payload TEXT NOT NULL
);

CREATE INDEX replica_pending_playlists ON replica_outbox(playlist_id,echoed,ordinal);
CREATE TRIGGER replica_operation_immutable BEFORE UPDATE OF ordinal,operation_id,playlist_id,payload ON replica_outbox BEGIN
    SELECT RAISE(ABORT, 'Replica operation identity is immutable');
END;
