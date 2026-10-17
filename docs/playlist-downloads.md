# Playlist downloads

Playlist downloads are implemented in the development branch through migrations 31 and 32. They are not deployed to the normal local server, which still runs migration 29. The [public-release plan](release-plan.md) remains the required scope.

Selecting **Download** retains the current playlist on the backend host. Reading a playlist, following it, importing it, or playing a track does not request a persistent copy. Web copies stay on the server; listening through the browser still requires that server connection.

## Ownership

```mermaid
flowchart LR
  UI[Account-scoped playlist workflow] --> HTTP[Authenticated HTTP adapter]
  HTTP --> Root[Immutable user retention root]
  Follow[Authoritative playlist entries] --> Reconcile[Joined core reconciliation]
  Root --> Reconcile
  Reconcile --> Transfer[Account / recording / backend-host transfer]
  Direct[Independent recording retention] --> Transfer
  Transfer --> File[Verified private managed copy]
  File --> Catalog[Authorized playback]
```

Core owns the SQLite transactions, download roots, reference checks, worker admission and file lifecycle. The host joins reconciliation and transfer workers. HTTP maps typed requests and results. The React workflow owns its requests, cancellation and status polling below the playlist page, so download updates do not rerender the track table.

A playlist retention root records one explicit user choice. The transfer and file are separate objects. Duplicate occurrences, multiple downloaded playlists and an independently downloaded recording can retain the same account's copy on the same backend host. Another account does not acquire that copy through its playlist.

Live playlist membership controls transfer and playback authority before the background projection catches up. A Follow refresh can make an existing occurrence playable or unavailable without changing its entry UUID. Reconciliation compares the recording mapping as well as the entry identity.

Removing a root withdraws only that retention. Other current roots keep their copy. After the last reference disappears, core retires the transfer and reclaims its managed bytes. It preserves original music files, library records, private metadata, likes, listening progress and independent playlist copies. Removed roots remain tombstones, so retrying an old removal cannot remove a later root.

## Commands and recovery

| Request | Result |
| --- | --- |
| `POST /api/v1/playlists/{id}/download` with a request UUID | `202` with the request, retention and playlist identities |
| `GET /api/v1/playlists/{id}/download` | Owned retention status, membership revisions and a bounded entry page; `404` when absent |
| `DELETE /api/v1/downloads/{retention_id}` | `204` after accepting removal of that root |

OpenAPI and TypeScript come from the actual Rust routes and wire types. Status defaults to 32 entries and accepts at most 100. Its cursor belongs to one retention and membership revision; changed membership requires restarting pagination. Entry states distinguish pending reconciliation, waiting for a source, queued/running/verifying work, ready copies, failed transfers and unavailable recordings.

An immutable receipt binds a request UUID to its original command. Replaying it returns that receipt, including after removal, without starting another transfer. Legacy recording receipts retain their original identities. New recording receipts use the existing `intent_id` wire field for the user retention identity; clients must treat it as the removable root rather than an internal transfer ID.

**Retry failed downloads** sends a fresh explicit playlist command while retaining the same root. Acceptance captures the terminal failures that command may retry. Each batch checks current authoritative membership; an unrelated playlist edit does not cancel the accepted retry. Duplicate entries cannot repeatedly restart a replacement that fails later. Replaying the accepted retry UUID also cannot restart that newer failure. Another attempt needs another explicit command. Durable waiting for a temporarily unavailable source remains under correction and verification.

An uncertain browser acknowledgement preserves the command UUID for **Retry download request**. Removal similarly retains its displayed root identity. Account changes and unmounting abort requests; late responses cannot publish into another workflow.

The UI checks only the first 32 entries and labels partial counts as checked entries. It does not infer that the whole playlist is saved from that page. Changed progress permits a 30-second window of one-second checks; unchanged work then uses 30-second checks. Focus and explicit actions can refresh the bounded fast window.

## Bounds and verification

Persistent reconciliation uses bounded transactions and durable cursors. Recording cursors skip duplicate tails during explicit retry. Membership changes coalesce through an outbox; retained ready copies wait for relevant root or membership changes rather than periodic rescanning. Transfers retain the existing limits of two global workers, one per account, 256 pending jobs globally and 32 per account. Playlist reconciliation has a separate joined SQL worker.

Root-run temporary SQLite and TCP tests cover Follow additions and withdrawal, stable occurrence identity, shared roots, stale removal, unprojected publication cleanup, quiescent retained copies, failed transfer recovery and immutable retry receipts. They verify actual managed audio and original/private-state preservation. The retry query diagnostic checks the production SQL with 100,000 duplicate entries and 20,000 unrelated failures under absent, inherited and fresh statistics. Those query checks are not a populated application benchmark or a release latency result.

The React suite passes 178 tests across 56 files, both TypeScript checks and the production build. Real page tests verify fresh failed-download retry, one request despite a double click, exact UUID recovery after a lost acknowledgement, same-root completion and unchanged playback. The final independent frontend review found no additional security, correctness or performance issues through source inspection. Browser verification against the real server remains pending.

Before final backend review fixes, the combined core/server suite passed 338 unique cases and 342 execution records, including four child-process repetitions, with one usual ignored fault test. Strict Clippy then found two reconciliation guard style errors, which were corrected separately. The final backend review identified lost retries after membership changes, consumed retries while waiting for a source, and cleanup work growing with retained transfer history. The membership regression now passes. Source waiting and the cleanup query still require their fixes and direct verification. Final combined checks, release-image browser verification and stopped backup/migration restoration remain acceptance work. No third independent backend review is claimed.

Local-original Download behavior, temporary playback-cache policy, disk reservations and exhaustion, forced crashes, broader source/provider races, Windows private ACLs, all desktop platforms and the release performance/codec gates remain open. Playlist downloads do not automatically match Spotify metadata to YouTube audio.
