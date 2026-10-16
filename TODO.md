# Hito TODO

Updated 2026-10-16. This is the remaining implementation work, not a list of features claimed complete. The full requirements and release gates are in [the release plan](docs/release-plan.md).

Multiple music folders are implemented in administrator Settings, with independent scans and removal/reconnection. Development setup launchers use native folder choosers and generate read-only Docker mounts without requiring `HITO_MUSIC_FOLDER`. Their live platform acceptance is still pending. Recording identity is separate from tagged-file checksums. Track and album details now support private title and artist edits with local source provenance. The server/web build is running in Docker; most release features below remain unimplemented.

## Library and metadata

- [x] Separate recording identity from tagged-file checksums so verified tag-only edits preserve playlists, likes, and progress. Preserve uncertain legacy references without guessing from paths.
- [x] Add artist/album pages, exact library-scoped credits, release appearances, tagged order, Unicode search, and release artwork.
- [x] Expose local source metadata observations, supported tag origins, truthful upgrade baselines, and bounded revision history.
- [ ] Add provider-backed artist, release, and recording matching without collapsing uncertain identities.
- [x] Show artist pages for provider recordings without releases, using recording credits without inventing albums.
- [ ] Add configurable MusicBrainz/provider enrichment and a disable setting.
- [x] Add personal recording titles, inheritance reset, synchronized acceptance, and consistent search/album/collection reads.
- [x] Add personal recording artists and album titles/artists with account-private grouping and field resets.
- [ ] Add unlinking, merge splitting, undo, administrator promotion, and reconciliation after raw release retags.
- [x] Add multiple server music folders, independent rescans, safe removal/reconnection, and administrator folder selection in Settings.
- [ ] Add filesystem watching, automatic desktop Music-folder discovery, and native desktop folder selection.
- [x] Add account-owned HTTP plugin connections, encrypted optional bearer tokens, administrator-offered service addresses, manifest review, and local removal.
- [ ] Complete provider-specific credential flows and shared provider defaults; add optional local PIN locks.

## Playback and desktop

- [x] Add a standalone Tauri development host with private local backend/storage, joined shutdown, and an isolated Linux WebView workflow.
- [x] Establish an owned-file native audio engine with Symphonia decoding and isolated CPAL monitor checks for playback, controls, and cleanup.
- [x] Establish session-bound core playback grants with durable membership/provider revocation and cancellation of pending opens.
- [x] Add an owned native session with authorized local/provider input, bounded range continuation, independent output revocation, and cleanup after canceled observers.
- [x] Add bounded ordered recording lookup, retained-session write guards, cooperative provider refresh cancellation, and terminal native startup cancellation for application coordination.
- [x] Own authenticated command admission and bounded paged queues with exact session/lease checks, stable occurrence IDs, revision/sequence fencing, and accepted-command receipt replay.
- [x] Drive authorized native output through that owner, with play/pause, separate position observations, natural queue advancement, same-session reattachment, account takeover/logout, and joined replacement/shutdown.
- [x] Recover native playback through finite authorized source attempts, one retained provider refresh, position restoration, paused recovery, and queue advancement after exhaustion.
- [x] Add ordered native seek/volume commands, completed-seek observations, explicit zero/backward restoration, and control supersession.
- [x] Persist observed native progress with ordered idempotent checkpoints, bounded backpressure, account isolation, and joined writes.
- [x] Expose authenticated native commands, paged queues, separate private observation streams, and retained desktop shutdown with library ownership.
- [x] Connect the shared React player to the native command/observation API while preserving the browser implementation. Verify actual Linux Tauri output, reload, controls, and Rust-owned progress.
- [x] Add native Previous using bounded played-occurrence history, stable duplicate entries, restart semantics, queue-end availability, and preserved pause state.
- [x] Add native shuffle and repeat with stable queue occurrences, finite failure traversal, and matching browser traversal checks.
- [ ] Add native playback speed and saved-position loading.
- [x] Preserve process-local private queue state across temporary authority/database failures while suspending output and rejecting unverified commands. Revalidate before recovery; clear on definitive session rejection.
- [ ] Build Tauri clients for macOS, Windows, and Linux with bundled local backend lifecycle.
- [ ] Add Symphonia decoding, CPAL output, FFmpeg fallback, and operating-system media controls.
- [ ] Add verified HTTP sources, preference ranking, manual override, server relay, direct fallback, conversion, and bitrate settings.
- [ ] Complete failover near the current position, deferred-source handling, gapless playback, and codec compatibility fixtures.
- [ ] Verify local audio opens, metadata, and ongoing reads on stalled or disconnected drives.
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
- [x] Add authenticated provider search/adoption APIs and bounded private audio relay with byte ranges and live cancellation.
- [x] Add browser discovery, explicit save/adoption, and playback through authorized server-relayed provider sources.
- [x] Add source-only provider refresh, bounded browser failover near the requested position, and explicit track-details availability checks.
- [ ] Complete remaining plugin capability requests/results, conversion, and operational capability negotiation.
- [ ] Ship Real-Debrid, AllDebrid, Premiumize, and TorBox adapters with preparation progress.
- [ ] Ship separately deployed music-folder, Jamendo, and yt-dlp reference plugins.
- [ ] Make the first reference plugin runnable in Docker with demo audio and documented UI installation; notify the user here when installation, discovery, adoption, playback, and seeking are verified.
- [ ] Give the folder plugin durable recording and release identities across tag edits and catalog membership changes; its first immutable demo uses content identities and requires a restart to rescan.
- [ ] Add playlist Import a copy and read-only Follow playlist.
- [ ] Import Spotify playlists as editable Hito copies, preserving order, duplicate entries, Spotify identifiers, and metadata provenance; verify current Spotify authorization and API restrictions.
- [ ] Follow Spotify playlists with a UI-configured hourly/daily refresh interval and persistent scheduled jobs; synchronize additions, removals, order, and duplicate occurrences while retaining stable entry IDs and the last good snapshot after failed or incomplete refreshes. Followed playlists stay read-only; imported copies stay independent and editable. Release unneeded download references without deleting originals or library records.
- [ ] Play imported Spotify playlist tracks through the yt-dlp YouTube plugin, with visible unavailable tracks and review of uncertain matches rather than silent substitutions.
- [ ] Add podcast RSS subscriptions, updates, progress, and downloads.
- [ ] Add audiobook grouping, chapters, bookmarks, speed, and resume positions.
- [ ] Add timed-lyrics and optional backing-track contracts and rendering.
- [ ] Run reproducible fixture tests and live checks for every official provider.

## UI and branding

- [ ] Regenerate Hito's logo and wordmark with a new visual direction, replace the current orange waveform across the app, and verify readability at small sizes and as an application icon.

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

The latest Docker verification and its limits are recorded in [current status](docs/status.md). Authenticated HTTP and the desktop lifecycle now expose the Rust owner for native output, source recovery, and ordered progress writes. The shared UI uses Rust playback in desktop and browser audio on the web. Physical/platform acceptance and the public-release gates remain open.

The latest full server/web Docker verification passed 247 unique default Rust cases, the separate enumeration fault test, 122 frontend cases, five Chromium workflows, generated contracts, zero audit vulnerabilities, and setup CLI checks. Native authority fault fixtures also pass. Complete desktop evidence and overlap are recorded in the status checkpoint. Browser evidence covers folder administration, artist/album navigation, private metadata editing with continuous audio, plugin discovery/installation, account isolation, real expired-range recovery, and availability checks. See [current status and verification limits](docs/status.md).

The normal persistent server is healthy on migration 0023 with the retained-session write guards deployed. Its latest stopped-volume audit preserves definitions, rows, and row IDs across all 64 tables, the credential key, and the host identity; database, foreign-key, and six FTS integrity checks pass. Provider discovery, authorized audio relay, recording-credit artist pages, source-only refresh, bounded browser recovery, and explicit availability checks are deployed locally. Prior upgrade audits preserve the private credential key, legacy file evidence, recording rowids, observation sequence, and membership backfill as well as the catalog and collection data below. Seven existing source metadata rows have truthful legacy observations; raw tag detail arrives on the next ordinary scan. Read-only comparison against stopped-volume backups preserves accounts, recording/source IDs, file checksums, playlists, entries, likes, progress, membership, and backend identity. All seven source-credit links remain valid. The account catalog contains two artists, two releases, seven source-credit relations, and seven effective recording credits. Folder tests cover authorization, containment, reconnects, host ownership, cancellation, expired attempts, and owned playback. Root-open fault injection uses the normal Linux backend; directory-enumeration injection uses rustix's libc backend and does not prove production raw-syscall outage behavior. See [folder behavior and limits](docs/music-folders.md).

Recording identity fixtures cover ten containers. Query-plan checks on 100,000 duplicate locators avoid temporary sorting. A Catalog API smoke test guards compilation-heavy artist and recording reads with 100,000 synthetic tracks before and after ANALYZE. A fresh-connection SQLx migration test backfills one million recording credits across ten accounts within a deterministic work budget, with and without statistics. These are not release latency benchmarks. [Catalog browsing](docs/catalog-browsing.md) documents exact credits, source appearances, Unicode search, and paged playback limits. An isolated production container exits successfully on Docker stop with lifecycle logs. Full backup restoration, platform acceptance, and public release gates remain unfinished.
