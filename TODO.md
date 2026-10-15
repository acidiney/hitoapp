# Hito TODO

Updated 2026-10-15. This is the remaining implementation work, not a list of features claimed complete. The full requirements and release gates are in [the release plan](docs/release-plan.md).

Current task: preserve recording identity across tag-only edits. Next: administrator UI for multiple music folders, their scan status, and library access. Administration belongs inside Hito Settings. The server/web foundation is running in Docker; most release features below remain unimplemented.

## Library and metadata

- [ ] Separate recording identity from tagged-file checksums so retagging preserves playlists, likes, and progress.
- [ ] Add artist/album pages, metadata provenance, and conservative recording matching.
- [ ] Add configurable MusicBrainz/provider enrichment and a disable setting.
- [ ] Add personal metadata edits, unlinking, merge splitting, undo, and administrator promotion.
- [ ] Add additional music folders/drives, rescanning rules, and automatic desktop Music-folder indexing.
- [ ] Add personal provider credentials, personal plugin installation, shared administrator defaults, and optional local PIN locks.

## Playback and desktop

- [ ] Build Tauri clients for macOS, Windows, and Linux with bundled local backend lifecycle.
- [ ] Add Symphonia decoding, CPAL output, FFmpeg fallback, and operating-system media controls.
- [ ] Add verified HTTP sources, preference ranking, manual override, server relay, direct fallback, conversion, and bitrate settings.
- [ ] Complete failover near the current position, deferred-source handling, gapless playback, and codec compatibility fixtures.
- [ ] Verify metadata compatibility, including a bounded policy for compressed ID3 tags, which are currently unsupported.
- [ ] Complete queue reordering and playback across catalog pages; verify very large queues against the performance gates.
- [ ] Verify that desktop works without a server and web explains unavailable connections.

## Synchronization and downloads

- [ ] Build a durable local replica and offline operation queue, reconnect handling, and account isolation.
- [ ] Add previewed standalone-library import while preserving its separate database.
- [ ] Verify concurrent edits, duplicate delivery, deletions, permissions, and device-local availability across replicas.
- [ ] Add explicit track/playlist downloads, persistent transfer jobs, and progress.
- [ ] Keep followed downloads synchronized with reference counting; preserve originals and library records.
- [ ] Separate temporary playback cache from persistent downloads; handle disk exhaustion and interrupted transfers.

## Providers and content

- [ ] Implement the language-neutral HTTP plugin protocol and capability negotiation.
- [ ] Ship Real-Debrid, AllDebrid, Premiumize, and TorBox adapters with preparation progress.
- [ ] Ship separately deployed music-folder, Jamendo, and yt-dlp reference plugins.
- [ ] Add playlist Import a copy and read-only Follow playlist.
- [ ] Add podcast RSS subscriptions, updates, progress, and downloads.
- [ ] Add audiobook grouping, chapters, bookmarks, speed, and resume positions.
- [ ] Add timed-lyrics and optional backing-track contracts and rendering.
- [ ] Run reproducible fixture tests and live checks for every official provider.

## Community service

- [ ] Build and deploy the separate source-ranking/correction API.
- [ ] Add separate disabled-by-default opt-ins for playback reporting and metadata corrections.
- [ ] Validate and redact contributions; rank successful playback and filter by configured providers.
- [ ] Add correction suggestions, provider validation/maintainer review, moderation, and rate limits.
- [ ] Verify playback continues during community-service outages.

## Release verification and distribution

- [ ] Benchmark 100,000 tracks, 10 accounts, and 5 simultaneous listeners on documented hardware/network conditions.
- [ ] Measure p95 local start, ready remote start, and available-source recovery against the release targets.
- [ ] Exercise slow providers, expired links, failed seeks, interrupted transfers, server outages, and recovery.
- [ ] Verify backups/restoration, migrations, upgrades, installation, and approval-based updates.
- [ ] Publish server containers/binaries, Compose deployment, Linux packages, and signed macOS/Windows installers.
- [ ] Add guided installation with native folder selection and automatic Docker read-only mount configuration; manual `HITO_MUSIC_FOLDER` setup is an advanced option.
- [ ] Complete platform verification on all three desktop systems.
- [ ] Provision community hosting/DNS and provider test accounts.
- [ ] Protect signing credentials, configure Apple notarization, and obtain Windows signing through SignPath or an agreed alternative.

Mobile, browser offline operation, social/group listening, arbitrary plugin UI, advanced recommendations, and karaoke recording/scoring remain outside this release.

## Verified foundation

- [x] Split Rust into identity, catalog, collection, and indexing services with private SQLite ownership.
- [x] Split React into explicit typed pages, components, and cancellable workflow hooks.
- [x] Generate OpenAPI from actual Rust routes and TypeScript from that schema.
- [x] Add correlated backend and local browser logging with redaction tests.
- [x] Align Docker and editor Rust toolchains. Verify real `tracing::instrument` expansion through rust-analyzer.
- [x] Fix trace-level extractor logs exposing rejected input.
- [x] Invalidate private state and history across browser tabs when accounts change, including delayed JavaScript initialization.
- [x] Fix the race between scan enqueue and a worker finishing that scan.
- [x] Fix stale Play errors and retain playback speed when changing sources.
- [x] Prevent the queue from rerendering on every playback position update.
- [x] Replace the initial table-first interface with a recording spotlight, artwork grid/list switch, grouped navigation, playlist cards, listening sidebar, expanded player, and tabbed settings.
- [x] Read authorized embedded/folder artwork with neutral fallbacks, bounded readers, and private no-store responses.
- [x] Verify desktop/phone layouts, native modal focus, queue-empty focus fallback, visible playback failures, and settings keyboard navigation in Docker.
- [x] Rerun Docker formatting, tests, Clippy, generated-contract checks, web build, audit, and browser workflow after the fixes.
- [x] Preserve legacy local-file ownership when upgrading an existing database without a device identity sidecar, then rebuild the normal Docker deployment.
- [x] Handle Docker SIGTERM through graceful shutdown and verify lifecycle logs.
- [x] Update architecture, operations, logging, status, and independent-review evidence.

Current evidence: Docker checks pass with 37 Rust tests, 30 browser unit tests, generated-contract checks, formatting, Clippy, production build, audit, and the browser workflow. Browser coverage includes the redesigned composition, native modal focus and errors, settings keyboard navigation, multiple tabs, and delayed JavaScript across account changes. Desktop and phone screenshots were inspected. The normal persistent server is rebuilt and healthy with its existing database and accounts; a stopped-volume backup preserves the pre-upgrade data. An isolated production container exits successfully on Docker stop with worker/server shutdown logs. The public release remains unfinished.

