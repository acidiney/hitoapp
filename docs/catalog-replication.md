# Catalog replication

Catalog replication is being built for desktop remote mode. A desktop must keep its standalone library separate from the server profile and remain usable when the server is unavailable. The [release scope](release-plan.md) includes catalog changes, populated playlists, likes, progress and source availability. The snapshot capability described here is an initial local-catalog boundary, not complete remote mode.

## Snapshot ownership

Core owns the catalog read, authorization, SQLite connection, materialization and retained pages. HTTP and desktop adapters must consume its typed results without querying SQLite. The snapshot contains inherited recording fields and source-specific title, artist, duration and release appearances. Recording, source and release identities retain their existing string values.

```mermaid
flowchart LR
    Caller[Authorized caller] --> Capability[CatalogReplication]
    Capability --> Reader[Owned SQLite reader]
    Reader --> Facts[Local recording and source facts]
    Facts --> Pages[Bounded immutable pages]
    Pages --> HTTP[Authenticated versioned HTTP]
    HTTP --> Consumer[Replicas private catalog installation]
    Authority[Session, PIN and library grant] --> Capability
    Stop[Cancellation and host shutdown] --> Reader
```

A source's `server_relay` value is an observation of whether the server can relay it. It does not make that file local or offline-ready on another device. Facts exclude filesystem coordinates, folder labels, credentials, raw tags and provider stream URLs. The authorized inherited display title remains catalog data even when indexing derived it from a filename.

The separate [retained playback lease capability](playback-leases.md) pins existing server source authority for audio opens. It does not map cached profile facts into ordinary library rows or complete replica playback. Its HTTP/client delivery and lost-creation-response resumption remain unfinished.

`begin` returns a snapshot identity and metadata only after successful construction and a final authority check. Repeating it for the same account, library and current grant resumes the same immutable snapshot without rebuilding it or extending its expiry. It can resume even when the retained registry is full. `page` reads an ordinal from that materialization. Each read validates its requesting session, PIN state and current library-grant identity. Snapshots belong to the account and library, so another currently authorized session of the same account can read them. Revoking and recreating library access does not restore access to an old snapshot.

`release` requires a current unlocked session. It removes only that account's snapshot and allows owned cleanup after library access is revoked. Unknown, expired and foreign handles are idempotent; a foreign caller cannot remove the owner's snapshot. Account erasure retires its constructions and removes retained pages. Host shutdown closes the capability and joins its reader.

## HTTP delivery

The versioned routes use shared authentication, origin policy and Core wire types. HTTP owns no SQL, retention or synchronization rules.

| Method | Route                                            | Successful response                                                            |
| ------ | ------------------------------------------------ | ------------------------------------------------------------------------------ |
| POST   | `/api/v1/catalog/snapshots`                      | `200 CatalogSnapshot`, accepting the strict shared `BeginCatalogSnapshot` body |
| GET    | `/api/v1/catalog/snapshots/{id}/pages/{ordinal}` | `200 CatalogSnapshotPage`                                                      |
| DELETE | `/api/v1/catalog/snapshots/{id}`                 | Empty `204`                                                                    |

Pages are compact JSON with no additional envelope and retain the 256 KiB encoded-page bound. Responses use the existing private, no-store policy. Malformed path fields return a safe JSON error, and invalid or inaccessible page handles return not found. Native bearer requests may omit Origin under the existing policy; browser cookie mutations still require origin verification. OpenAPI and TypeScript are generated from the actual Rust routes and types.

If the caller loses a successful creation response, it can repeat POST to recover the same handle and facts until expiry or release. Release followed by POST constructs a fresh view. These transient server handles do not provide durable client storage or incremental changes.

## Durable private catalog cache

`Replicas.install_catalog` receives the existing snapshot types through the bounded native transport. It stages validated facts in the connection's private SQLite profile, then publishes one library head atomically. `catalog_snapshot` and `catalog_page` read that committed generation under the current local session and connection lease. These are Core APIs; desktop profile selection and rendering are not implemented by this slice.

```mermaid
flowchart LR
    Server[Server snapshot API] --> Transport[Bounded authenticated transport]
    Transport --> Validate[Validate identity, order and limits]
    Validate --> Stage[Private profile staging generation]
    Stage --> Commit[Atomic library head publication]
    Commit --> Read[Authorized offline catalog reads]
    Authority[Local session, PIN and connection generation] --> Stage
    Authority --> Read
    Stop[Caller drop, retirement or shutdown] --> Owner[Retained installation owner]
    Owner --> Stage
    Owner --> Settle[Settle commit or rollback before releasing admission]
```

Migration 0094 adds private installation, recording-fact and library-head tables. Facts keep their exact recording, source, observation and release identities, including legacy string values. Source-specific entry, disc and track fields remain distinct. Duplicate recording/source identities, conflicting release-level fields, repeated release-entry identities and a snapshot UUID shared by different cached libraries are rejected. No ordinary recording/source rows, filesystem coordinates, audio, provider bindings or metadata provenance are fabricated.

Each page is validated and written in a short transaction without a network wait. The old head remains readable until all pages, ordinals, identities and totals match. An incomplete or failed refresh preserves the committed cache. A later installation or profile reopening removes abandoned staging and superseded generations. Empty catalogs retain the producer's one empty terminal page.

The retained owner holds per-connection synchronization admission and one of four global network slots. Dropping the public caller signals cancellation; the owner settles SQL and removes its own work entry independently of observer polling. The final control guard stays held until the profile commit or queued rollback has settled. Deactivation, disconnect, erasure and shutdown join retained work before retiring its profile. Publication is the atomic boundary: after it succeeds, a later error or observer drop can leave a complete new generation.

The existing per-export limits apply to actual received page bytes, not just reserialized JSON. The retained owner uses a two-minute success budget; SQL settlement can outlast it. A profile admits at most sixteen cached libraries and 256 MiB of page-wire bookkeeping, including old and staging generations. These are logical admission limits, not bounds on physical SQLite/WAL size or total process memory. Explicit downloads, original media and the standalone library are untouched.

The server's `expires_at` limits its transient lease. It does not expire a committed offline cache. A current authorized local lease can read the last known catalog while the server is unavailable. Remote permission or session failure stops refresh and preserves that cache; reauthentication retires the old local activation. The protocol has no remote grant incarnation or revocation feed and cannot promise immediate offline erasure. Local PIN/session/account checks still apply on every read, and local account erasure retires the owned profile through the existing file ledger.

The snapshot's collection watermark never advances a collection cursor, acknowledges an operation or changes the outbox. Catalog installation does not add populated playlists, likes, progress, local audio availability or incremental catch-up.

## Resource and cancellation policy

Only one constructor runs at a time. An export is limited to 100,000 recordings and 128 MiB of encoded pages, with at most 128 local sources per recording. Each fact is bounded to 64 KiB; a page contains at most 200 facts and 256 KiB. The retained registry allows at most ten snapshots and 256 MiB of encoded data, including a full export reservation while construction is active. These are export budgets, not a bound on the application's total memory.

A constructor has one ten-second success deadline beginning at public entry. It cannot publish a ready snapshot after that deadline. Cancellation retires the observer, while the retained owner continues to settle SQLite work and close its connection. Cleanup can outlast the success deadline; admission remains held until it finishes. An idle reader waiter is cancellable without acquiring the connection. A successful construction keeps the reader for reuse, and capability close joins active construction before gracefully closing the reader.

Snapshots expire after five minutes and are transient. Restart invalidates their handles. There is no durable snapshot file or new database migration in this slice.

## Qualification

Docker qualification on 10 November 2026 passes six unit cases for SQL work, scale and retained deadline ownership, one public snapshot tracer and five public authority/immutability cases. The shared runtime case proves snapshot retirement while HTTP is still draining. Existing account-deletion, replica and real Router regressions also pass. An independent repeat covered the six unit cases, six public snapshot cases, eight account-deletion cases, nineteen replica cases and the runtime case against unchanged source. These overlapping component counts are not a release total.

The scale fixture contains 100,000 recordings, distinct release identities, source-metadata rows and valid local-file coordinates, with foreign keys enabled and checked. One owned WAV supplies the only ready relay observation; the remaining coordinates represent unavailable historical files. Every recording identity is read through 500 public pages, totaling 52,450,811 encoded bytes. This is a synthetic database export test, not a 100,000-file scan or listening-load gate.

Absent, stale and fresh planner statistics each require about 24.1 million SQLite VM steps, below the fixed 60-million construction budget. In the independent run, construction takes 3.26 to 3.35 seconds on an Apple M5 Pro with 24 GiB RAM and 15 logical CPUs, using Docker 29.4 Linux arm64 and a 4 GiB tools container. Debug information and incremental compilation are disabled. These are three warm development observations, not p95 or cold-storage measurements.

Review exposed repeated scans under stale statistics. Snapshot queries now use bounded recording batches and existing indexed identity lookups. A corrected regression verifies that recordings, releases, metadata and file coordinates all grow, rather than silently ignoring invalid fixture rows. To isolate export from an existing source-artist write-trigger problem, the fixture constructs data with triggers and foreign keys enabled, then restores the exact captured one-row planner statistics before measuring the actual snapshot reader. It preserves and checks both statistics tables when present. This does not qualify the separate stale-statistics write path.

Controlled deadline cases verify that a held publication writer cannot publish late, a reader waiter can expire without acquiring its slot, and close retains the reader until in-flight SQL settles. Public cases cover another account, a second current session of the owner, revoked sessions, PIN locking, revoke/regrant, retained source-specific release facts after rescan, and invalid handles after close. Fresh reviews found and resolved query-work, deadline and test-cleanup defects. Strict scoped checks and formatting pass. No running server or desktop package is updated by this slice.

The create-or-resume and release extension has an independent 26-case Docker repeat covering public snapshot/authority/lifecycle behavior, retained SQL work and deadlines, and actual server runtime closure. It also caught and corrected retention-clock handling after expiry and reservation lifetime during SQL cancellation. The synthetic 100,000-recording runs remain about 3.34 to 3.37 seconds in that repeat; these are warm development observations.

The HTTP adapter passes ten actual Router/TCP cases, independently repeated in 6.49 seconds. They cover strict and bounded requests, session/PIN/account/grant isolation, browser/native origin policy, immutable retry after rescan, owned and foreign release, original expiry, fresh construction, and escaped pages near 256 KiB. The TCP case reads a committed snapshot identity before dropping the socket, then verifies the same retained facts and joined server shutdown. It proves retry after publication and partial client consumption, not prepublication cancellation or an interrupted server write. Existing collection state remains unchanged.

Three fresh source reviews cover security, correctness and resource/recovery ownership. Strict server all-target Clippy, formatting, both TypeScript checks and independent byte-for-byte regeneration of OpenAPI/TypeScript pass. The generated contract adds three paths and eight shared schemas, preserving existing entries. These checks supplement the earlier combined 261-case run; they are not a new full-suite or release result.

The durable-cache extension passes 30 public replica cases, nine account-deletion cases and 30 protocol cases. Actual Router acceptance passes nine cases, including three new catalog workflows. The latter install 47 owned indexed WAVs across near-256-KiB pages, compare every source/release fact after a joined client/server shutdown and offline restart, preserve standalone originals and exact queued operations, and verify refresh, permission, session, PIN and account isolation. Server-relay observations remain server-scoped.

Independent integrated verification also passes all 158 Core unit cases and the selected snapshot, authority, lifecycle, backup, source-upgrade and server-shutdown groups. It found a historical playlist fixture whose supported endpoints stopped at schema 93. The correction explicitly supports 94 without weakening typed-row, rowid, FTS, schema or migration-ledger assertions. A targeted 33-case repeat covers the changed profile fixture, stopped backup and both playlist-upgrade cases. Three fresh specialist reviews pass both rounds. Core all-target and scoped server strict Clippy, workspace formatting and unchanged OpenAPI export pass on the final source. These overlapping checks are not a complete release suite.

The cancellation tests exercise twelve dropped callers, a blocked profile writer, actual deferred-foreign-key commit failure and a held SQLite rollback hook. Admission and the control guard remain held until the worker settles, the retained owner reaps its work entry, and the previous head remains readable. Quota tests count actual padded HTTP bytes at the export/profile limits without constructing one aggregate client buffer.

A separate stopped-data acceptance now restores a populated catalog profile in Linux Docker, with two exact pages, private ownership, indexed standalone metadata and immutable pending-operation preservation. It retains the original external audio location; audio relocation, hot backup and physical platforms remain open. See [stopped restoration](offline-replicas.md#stopped-backup-and-restoration). Client-install scale, measured SQLite/WAL growth and actual disk exhaustion are qualified below. Platform packages, remote audio and desktop profile selection remain unverified or unfinished. No running server or desktop package is updated by this source qualification.

## Client storage qualification

The Linux fixture serves 100,000 synthetic recording/source facts with distinct releases through an owned TCP peer. Installation consumes all 500 pages and 68,292,682 encoded bytes. It compares every fact before and after a joined offline restart. Standalone indexed audio, playlists and activity remain separate. A pending rename retains its exact body and identifier; reconnection sends it once with the original synchronization client identity and independent collection cursor.

Recorded warm development observations on the Apple M5 Pro, 24 GiB RAM, 15 logical CPUs, Docker 29.4 Linux arm64 and a 4 GiB tools container are:

| Observation                             |       Measurement |
| --------------------------------------- | ----------------: |
| Installation                            |          9,496 ms |
| Read and compare every offline page     |          2,624 ms |
| Logical owned files before installation |  12,451,745 bytes |
| Logical owned files after installation  | 110,801,641 bytes |
| Profile database after installation     | 102,121,472 bytes |
| WAL after installation                  |   8,598,504 bytes |
| Logical owned files after reopening     | 108,232,609 bytes |

The owned-file total includes control and profile databases and the fixture original. It is an observation, not a physical storage cap. This is a synthetic install test rather than a real-file scan, multi-listener test or p95 benchmark.

A separate explicitly enabled test guards that its directory is a dedicated 64 MiB tmpfs. Allocated regular-file writes reach ENOSPC, rather than a sparse-file or logical-quota limit. An 8,000-fact, 40-page refresh then fails with the safe public private-storage error. Every one of the 401 old facts remains readable while full and after reopening. Removing only the owned filler lets the same refresh succeed; every replacement fact survives another offline restart. The external sentinel, original audio, standalone state, pending operation, client identity and collection cursor remain exact. This does not qualify physical power loss or other operating systems.

Fresh review and an independent final-source repeat pass two normal cases and the explicit disk case, strict Clippy and formatting. Review corrected verification that could have recreated a lost pending operation and a client-identity comparison that previously started after reopening. The final checks inspect preserved state before reconnection and carry the original client identity throughout.

Run the isolated qualification with:

```sh
scripts/qualify-replica-catalog-storage.sh
```

The runner builds tools and fetches locked dependencies, then runs offline under the tools profile. `--offline` uses an already populated dependency cache. `HITO_CATALOG_STORAGE_TOOLS_IMAGE` selects an existing local tools image; `--help` documents optional dedicated volumes and the shared Cargo lock. Sources are read-only, the disk fixture uses ephemeral tmpfs, and no normal server volume is mounted. Each source snapshot receives a distinct Cargo target. Private ignored result directories contain phase logs and an exact-count TSV.

Independent execution qualified the prebuilt-image, populated-cache `--offline` path. It passed the two normal cases in 14.75 seconds and the disk case in 2.32 seconds. The default tools-build/fetch path has configuration and source review, but was not executed in that repeat. The resource guard measures discovered named Hito target volumes and known host targets before each phase. Custom targets and BuildKit caches still require separate inspection; see [development resource limits](development.md#disk-and-cpu-budget).

## Incremental replication remains open

The wire mode is explicitly `snapshot_only`. `collection_watermark` is an observation of the collection log during construction. It must never advance an existing collection cursor or acknowledge an offline operation. The collection log does not cover every writer of exported catalog fields, so it cannot yet provide catalog catch-up. The [selected incremental design](catalog-journal-design.md) records the intended journal, normalized facts, cursor privacy and acceptance boundary.

Durable snapshot installation is implemented at the private Core profile boundary. Incremental synchronization still needs a catalog change journal covering every exported-field writer and resumable change delivery. Populated playlist projection, likes/progress, remote audio ownership and desktop profile selection also remain open. Provider-only recordings require their own catalog coverage. Personal overrides and standalone import preview must remain separate. See [offline replica ownership](offline-replicas.md).

A producer audit identified the dependencies that incremental delivery must cover:

| Facts that change                                      | Producers to cover                                                                           |
| ------------------------------------------------------ | -------------------------------------------------------------------------------------------- |
| Local recording/source metadata and release appearance | Indexing, legacy recovery and metadata-owner selection                                       |
| Inherited recording/release fields                     | Local indexing and provider adoption, import, Follow and metadata repair on mixed recordings |
| Server relay observation                               | File withdrawal/recovery, folder connection/rebinding and account source choice              |
| Recording disappearance                                | Maintenance purge and account erasure, with durable tombstones                               |
| Cache authority                                        | Session/PIN validation and exact account/library grant incarnation                           |

The current raw snapshot does not export artist identities, recording-group membership or personal overrides. A journal covering the current fields would not establish replication of those wider catalog semantics. Indexing events alone are insufficient, and a dirty revision must not be cleared by acknowledgement of an older revision. The next contract must also handle snapshot/change races, release/folder fanout and expired history without changing the collection outbox or treating the snapshot watermark as its acknowledgement.
