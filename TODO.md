# Hito TODO

Updated 2026-10-15. This is the remaining implementation work, not a list of features claimed complete. The full requirements and release gates are in [the release plan](docs/release-plan.md).

Multiple music folders are implemented in administrator Settings, with independent scans and removal/reconnection. Development setup launchers use native folder choosers and generate read-only Docker mounts without requiring `HITO_MUSIC_FOLDER`. Their live platform acceptance is still pending. Recording identity is separate from tagged-file checksums. Track and album details now support private title and artist edits with local source provenance. The server/web build is running in Docker; most release features below remain unimplemented.

## Library and metadata

- [x] Separate recording identity from tagged-file checksums so verified tag-only edits preserve playlists, likes, and progress. Preserve uncertain legacy references without guessing from paths.
- [x] Add artist/album pages, exact library-scoped credits, release appearances, tagged order, Unicode search, and release artwork.
- [x] Expose local source metadata observations, supported tag origins, truthful upgrade baselines, and bounded revision history.
- [ ] Add provider-backed artist, release, and recording matching without collapsing uncertain identities.
- [ ] Add configurable MusicBrainz/provider enrichment and a disable setting.
- [x] Add personal recording titles, inheritance reset, synchronized acceptance, and consistent search/album/collection reads.
- [x] Add personal recording artists and album titles/artists with account-private grouping and field resets.
- [ ] Add unlinking, merge splitting, undo, administrator promotion, and reconciliation after raw release retags.
- [x] Add multiple server music folders, independent rescans, safe removal/reconnection, and administrator folder selection in Settings.
- [ ] Add filesystem watching, automatic desktop Music-folder discovery, and native desktop folder selection.
- [x] Add account-owned HTTP plugin connections, encrypted optional bearer tokens, administrator-offered service addresses, manifest review, and local removal.
- [ ] Complete provider-specific credential flows and shared provider defaults; add optional local PIN locks.

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

- [x] Implement the versioned HTTP manifest contract and private installation lifecycle.
- [x] Add core provider search and durable adoption with owner-only libraries, stable identities, and provider provenance.
- [ ] Expose provider discovery and selection through the application API/UI, then connect verified source resolution and server-relayed playback.
- [ ] Complete plugin capability requests/results, account-scoped provider content, source resolution/relay, and operational capability negotiation.
- [ ] Ship Real-Debrid, AllDebrid, Premiumize, and TorBox adapters with preparation progress.
- [ ] Ship separately deployed music-folder, Jamendo, and yt-dlp reference plugins.
- [ ] Add playlist Import a copy and read-only Follow playlist.
- [ ] Add podcast RSS subscriptions, updates, progress, and downloads.
- [ ] Add audiobook grouping, chapters, bookmarks, speed, and resume positions.
- [ ] Add timed-lyrics and optional backing-track contracts and rendering.
- [ ] Run reproducible fixture tests and live checks for every official provider.

## UI and branding

- [ ] Regenerate Hito's logo and wordmark, replacing the current waveform design across the app.

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
- [x] Add development setup launchers with native folder selection and generated Docker read-only mounts; manual `HITO_MUSIC_FOLDER` setup is an advanced option.
- [ ] Verify native setup dialogs, permissions, drive reconnects, project discovery/adoption, stopped backups, and rollback on macOS, Windows, and Linux; package the setup experience for distribution.
- [ ] Complete platform verification on all three desktop systems.
- [ ] Provision community hosting/DNS and provider test accounts.
- [ ] Protect signing credentials, configure Apple notarization, and obtain Windows signing through SignPath or an agreed alternative.

Mobile, browser offline operation, social/group listening, arbitrary plugin UI, advanced recommendations, and karaoke recording/scoring remain outside this release.

## Verified foundation

- [x] Split Rust into identity, catalog, collection, and library-storage services with private SQLite ownership.
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

Current evidence: Docker verification passes with 124 Rust workspace tests, one separately run enumeration fault test, 56 frontend tests, and two setup CLI tests. Formatting, Clippy, generated contracts, production build, audit with zero vulnerabilities, Compose path interpolation, and the Chromium workflow pass. The browser also verifies private plugin installation, manifest review, token replacement/clear, account isolation, removal/withdrawal, focus restoration, and phone forms against a real HTTP fixture. It adds two folders, navigates artists/albums, saves and resets recording and album title/artist fields with playback continuing, delays a refresh to protect drafts, and checks mobile layout, Escape/focus restoration, and account isolation. Desktop and phone screenshots were inspected. PowerShell syntax parses in Docker; native dialogs have not been accepted on their host platforms.

The normal persistent server is healthy after migration 0018. Seven existing source metadata rows have truthful legacy observations; raw tag detail arrives on the next ordinary scan. Read-only comparison against its stopped-volume backup preserves accounts, recording/source IDs, file checksums, playlists, entries, likes, progress, membership, and backend identity. All seven source-credit links remain valid. The account catalog contains two artists, two releases, and seven source-credit relations. Foreign keys, database integrity, and scoped search-index integrity pass. Folder tests cover authorization, containment, reconnects, host ownership, cancellation, expired attempts, and owned playback. Root-open fault injection uses the normal Linux backend; directory-enumeration injection uses rustix's libc backend and does not prove production raw-syscall outage behavior. See [folder behavior and limits](docs/music-folders.md).

Recording identity fixtures cover ten containers. Query-plan checks on 100,000 duplicate locators avoid temporary sorting. A Catalog API smoke test guards compilation-heavy artist reads with 100,000 synthetic tracks before and after ANALYZE. These are not release latency benchmarks. [Catalog browsing](docs/catalog-browsing.md) documents exact credits, source appearances, Unicode search, and paged playback limits. An isolated production container exits successfully on Docker stop with lifecycle logs. Full backup restoration, platform acceptance, and public release gates remain unfinished.

