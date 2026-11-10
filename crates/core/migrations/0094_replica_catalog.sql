CREATE TABLE replica_catalog_installs (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES users(id),
    library_id TEXT NOT NULL,
    snapshot_id TEXT NOT NULL,
    metadata TEXT NOT NULL,
    committed INTEGER NOT NULL DEFAULT 0 CHECK(committed IN (0,1)),
    bytes INTEGER NOT NULL DEFAULT 0 CHECK(bytes >= 0)
);
CREATE INDEX replica_catalog_account ON replica_catalog_installs(account_id);
CREATE TABLE replica_catalog_facts (
    install_id TEXT NOT NULL REFERENCES replica_catalog_installs(id),
    id TEXT NOT NULL,
    page_ordinal INTEGER NOT NULL CHECK(page_ordinal >= 0),
    position INTEGER NOT NULL CHECK(position >= 0),
    payload TEXT NOT NULL,
    PRIMARY KEY(install_id,id),
    UNIQUE(install_id,page_ordinal,position)
);
CREATE TABLE replica_catalog_heads (
    library_id TEXT PRIMARY KEY,
    install_id TEXT NOT NULL UNIQUE REFERENCES replica_catalog_installs(id)
);
