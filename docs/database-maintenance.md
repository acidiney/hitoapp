# Database maintenance

The server includes an explicit migration-only command, now included in the normal local development deployment. It exists so an upgrade can inspect migrated data before normal startup launches background work.

```text
hito-server migrate --database PATH --expect-version VERSION
```

`PATH` must identify an existing regular Hito SQLite database. `VERSION` must match its current, complete embedded migration prefix. Missing files, unrelated databases and mismatched histories are rejected. SQLx validates the migration checksums and applies the same embedded migrations used by normal startup.

Stop every application writer and take a whole-volume backup before using this command. Keep the SQLite database on local persistent storage. An exclusive SQLite connection does not establish that another Hito process has stopped, particularly with WAL or an idle connection pool. The operator must verify that separately.

```mermaid
flowchart LR
  Stop[Stop and verify application writers] --> Backup[Back up and verify the whole volume]
  Backup --> Migrate[Apply embedded migrations only]
  Migrate --> Inspect[Check stopped database and files]
  Inspect --> Runtime[Start and verify application runtime]
```

The command bypasses runtime configuration, web assets, folder initialization, device identity, plugin-key initialization, HTTP listeners and background jobs. Core owns the database connection and migration lifecycle. Canceling an observing future leaves that owned operation running through connection close. The CLI waits for completion when it receives its graceful shutdown signal.

SQLx applies migrations with its own transaction handling. A batch can stop after earlier migrations have committed. Do not infer that an error rolled back the entire upgrade. Inspect the actual migration history before retrying or restoring the stopped backup.

Success emits JSON with the previous and current versions, applied count and elapsed time. Failure emits a static error code and elapsed time. Output excludes the database path, raw database errors, credentials and library contents.

The real-binary test upgrades a populated migration-29 library through migration 35 while unavailable runtime assets and due scan, Follow and download jobs are present. It verifies no provider requests, preservation of prior typed application and FTS rows, noncontiguous transfer and receipt rowids, sequence high-water marks, migration checksums, private key and device-ID bytes, and original and managed audio bytes. It checks foreign keys and database integrity after the upgrade.

Nine additional real-binary cases verify missing, empty and unrelated files, wrong versions, checksum failures, invalid integer and text success markers, an exact-version no-op and selection of a literal URI-like filename beside another database. Failure output is checked against private path, title and token canaries. A separate public-capability test cancels the observer at a completed history-read barrier, then requires the owned migration's post-close outcome, preserved data and a fresh writable transaction. All 11 directed cases, strict core/server Clippy, default workspace checks and formatting pass.

These tests do not establish whole-volume filesystem metadata preservation, backup restoration, real lock contention, graceful CLI signals, platform acceptance or public upgrade readiness. Those checks remain separate release work. Native command-line path parsing also does not add non-UTF-8 filename support to SQLx.

## Stopped migration 0046 to 0048 rehearsal

On 2026-10-18, the bundled migration-only executable in image `sha256:ecfaee71be260f83131d9a3d513165f7b52fbcc3925023754c610b461c38e0f8` upgraded a separate restored clone from migration 0046 to 0048. The input was a retained, verified whole-volume archive taken while the normal server was stopped. This rehearsal never mounted or queried the normal volume and did not authenticate a listener account.

Archive headers and payload hashes matched the retained manifest before restoration into a new empty volume. The stopped baseline and migrated clone passed read-only audits. The migration ran as UID 10001 with networking disabled, no published ports, a read-only container root and write access only to the clone. Its JSON outcome reported two applied migrations and the process exited successfully. No server runtime or background worker started.

The audit preserved all 106 prior application tables, 12 prior views and 12,572 prior typed rows, including rowids, generated columns, sequence state, statistics and index/foreign-key metadata. All 46 earlier migration receipts and checksums remained unchanged. The only schema additions were four empty tables, four declared indexes, six implicit indexes and eight triggers, plus the two verified migration receipts. Integrity and foreign-key checks passed; six FTS integrity checks ran on separate writable audit copies.

Non-SQLite files, keys and host identity retained their exact contents, owners, modes and timestamps. Original database permissions were preserved rather than normalized; private audit copies use mode 0600. Source hashes and result hashes bind each accepted stage to the reviewed audit. All five owned containers exited with code zero, no out-of-memory termination or active clone writer remained, and the clone and private evidence were retained.

This establishes the stopped clone's restoration and migration-only preservation. Full runtime startup, active work, normal deployment, migration 0049 and packaged platform upgrades remain separate gates.
