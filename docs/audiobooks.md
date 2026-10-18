# Audiobook core foundation

Audiobook grouping and private bookmarks are qualified in core source. This is not a complete audiobook feature: dedicated list/read HTTP routes, shared pages and the audiobook playback workflow are not implemented. Migration 0048 is not deployed on the normal server. The full release scope remains in [release-plan.md](release-plan.md).

## Ownership and identities

An audiobook annotates an existing editable playlist owned by the account. Its identity is the backing playlist ID. It retains that playlist's name, order and chapter occurrence IDs, including two occurrences of the same recording. Grouping creates no recordings, sources or alternate chapter ordering.

`Collection::audiobook` reads the owned live playlist, its authorized chapter projection and private bookmarks in one transaction. Core owns the SQL, permissions and synchronization transaction. There is no audiobook forwarding service, provider request, playback engine or download owner.

A bookmark retains its account, book, bookmark ID, exact chapter entry ID, recording ID, position in milliseconds and label. It belongs to a particular occurrence rather than a matching title or recording elsewhere in the book. Another account's access to the same shared-library recording does not grant access to the book or bookmark. A live followed playlist is read-only and cannot be grouped; an owned editable copy is eligible.

Existing per-account, per-recording listening progress remains the sole owner of saved positions and playback speed. Grouping and bookmark changes preserve that state, personal title/artist overrides, likes, metadata observations, sources, other playlists and original audio. They start no audio or persistent download.

## Synchronization and deletion

The existing collection operation format carries these actions:

| Action | Fields |
| --- | --- |
| `audiobook_create` | `playlist_id` |
| `audiobook_bookmark_create` | `audiobook_id`, `bookmark_id`, `entry_id`, `recording_id`, `position_ms`, `label` |
| `audiobook_bookmark_remove` | `audiobook_id`, `bookmark_id` |

Exact operation replay returns its original acknowledgement without another event. A fresh accepted operation retains its original synchronization payload even when deletion makes it a visible no-op. Reusing a bookmark identity for a different live position conflicts.

Bookmark removal retains a tombstone in the same authoritative row. Removing an unknown marker records only its account, book and marker identity; it invents no chapter or recording. Later creation cannot resurrect that marker or move its identity to another book. Removing one marker preserves the others. Foreign book operations return `NotFound`; same-account cross-book identity conflicts do not become cross-account existence checks.

Deleting the backing playlist wins over delayed audiobook operations. Grouping, bookmark creation and removal on an owned deleted playlist are acknowledged no-ops, including in the same batch as deletion. They cannot roll back unrelated valid work in that batch or recreate the book. Public playlist and audiobook reads still return `NotFound`. Missing and foreign playlists remain denied. Bookmark input bounds still apply before accepting a deleted-book no-op.

## Bounds and migration

The chapter projection rejects books exceeding 10,000 live occurrences rather than silently truncating them. A book supports at most 256 active bookmarks. Positions are nonnegative integer milliseconds. Labels must contain text after trimming and fit within 256 raw UTF-8 bytes before trimming; whitespace padding counts toward that bound. These are byte limits, not character limits.

Migration 0048 adds the book annotation and bookmark rows, ownership/identity guards and indexes. Unknown-marker tombstones have no position payload; visible bookmarks require the complete chapter/recording/position/label payload. Historical migration inputs remain pinned to their original versions and checksums. Upgrade fixtures require the exact declared 0048 schema and empty new book tables, preserving prior typed rows, rowids, sequences, receipts, views, foreign keys, search state, private keys and original files.

The full `(audiobook_id, entry_id)` child index bounds foreign-key checks when ordinary Follow refresh deletes unrelated playlist entries. The work regression applies all actual migrations and seeds 100,000 retained markers across 400 owned books and ten accounts, with 125 active and 125 deleted markers per book. It closes and reopens SQLite under absent, inherited and fresh statistics, enables foreign keys, deletes 20 retired entries under `BEGIN IMMEDIATE`, then verifies exact retained state and rollback.

Before the index, the absent-statistics DELETE used approximately 6,000,900 SQLite VM steps and failed the 50,000-step budget. With the index, all three statistics states use approximately 1,000 steps. This fixture measures database work; it is not the release load test or a playback latency benchmark.

## Verification and remaining work

Six focused cases pass:

- Grouping, exact chapter and bookmark identities, private musical-state preservation and reopening.
- Account isolation, exact replay and bookmark anchoring after chapter reordering.
- Oversized raw-label rejection before insertion and before an existing-marker no-op.
- Bookmark removal, exact replay, later creates, remove-before-create tombstones and book/account boundaries.
- Backing-playlist deletion winning over all three actions while independent batch work commits, including after reopening.
- The real-schema foreign-key work budget described above.

The first five exercise public capabilities with real temporary SQLite and owned deterministic originals. The sixth exercises the actual database schema and parent DELETE. Independent security, correctness and performance reviews identified a P2 raw-label payload issue, a P1 parent-deletion rollback issue and a P1 missing child-index issue. Each received a failing regression before its minimal fix; the relevant second review verified the resolution. Strict all-target core Clippy and formatting pass.

The integrated full-core run passed 331 distinct cases, with one usual fault case ignored, and exposed six migration fixtures still expecting schema 0047. Their latest endpoints and exact additive schema expectations were corrected without changing historical inputs or preservation checks. All six directed targets then passed, qualifying 337 distinct core cases across those runs. This is not a single green run of all 337 cases.

The combined workspace also passes 148 distinct server cases, strict all-target workspace Clippy and formatting. The generated route/type export changes only the existing `Action` schema and reproduces byte for byte. All 298 frontend cases, both TypeScript configurations and the production web build pass with those generated types. These checks do not add dedicated audiobook pages or read routes.

The next slice is authenticated list/read adapters, generated contracts and a shared chapter-based workflow for grouping existing playlists, bookmark create/remove/jump, explicit chapter resume and speed controls. Chapter/bookmark limit boundaries, access revocation and removed-chapter bookmark presentation need further qualification. A retained marker must never silently retarget another occurrence.

A book-wide Continue button additionally requires an explicit selected-chapter anchor and collection occurrence provenance through browser and native queues. Recording timestamps or the current recording cannot identify a duplicate chapter occurrence. Automatic advancement, navigation, account switching and consistent resume must use the existing playback owner and per-recording progress, without another position store. These remain required release work, along with packaged browser, native/platform and normal-deployment acceptance.
