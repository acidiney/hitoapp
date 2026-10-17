# Hito TODO

Updated 2026-10-17. This is the remaining implementation work, not a list of features claimed complete. The full requirements and release gates are in [the release plan](docs/release-plan.md).

Multiple music folders are implemented in administrator Settings, with independent scans and removal/reconnection. Development setup launchers use native folder choosers and generate read-only Docker mounts without requiring `HITO_MUSIC_FOLDER`. Their live platform acceptance is still pending. Recording identity is separate from tagged-file checksums. Track and album details now support private title and artist edits with local source provenance. The server/web build is running in Docker; most release features below remain unimplemented.

Automatic audio resolution for imported playlists is deployed locally. Controlled browser workflows and one real Spotify-to-YouTube WebKit target pass without per-track URLs or manual matching. Existing YouTube connections must explicitly approve the added metadata capability. Manual source selection corrects a result; broader synchronization, matching, native and release acceptance remain open. See [current implementation, evidence and gaps](docs/automatic-sources.md). Visual polish remains after release functionality.

## Library and metadata

- [x] Separate recording identity from tagged-file checksums so verified tag-only edits preserve playlists, likes, and progress. Preserve uncertain legacy references without guessing from paths.
- [x] Add artist/album pages, exact library-scoped credits, release appearances, tagged order, Unicode search, and release artwork.
- [x] Expose local source metadata observations, supported tag origins, truthful upgrade baselines, and bounded revision history.
- [ ] Add provider-backed artist, release, and recording matching without collapsing uncertain identities.
- [ ] Fix duplicate artist cards and pages across local files and imported playlists. Reported example: C4 Pedro appears once with a local track/album and again with an imported track. Unify verified artist identities and aggregate their tracks/releases while preserving recording/source identities, playlist entries, likes, progress and personal edits. Keep distinct artists with the same name separate.
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
- [x] Add native 0.5–3× playback speed for supported 8–192 kHz mono/stereo sources, preserving pitch, command ordering and observed progress rate.
- [x] Load current authorized saved positions for explicit native History resume, with pending-write and stale-result fencing.
- [ ] Add seamless source-specific speed capability negotiation and conversion for unsupported representations; current nonunit playback above 192 kHz fails visibly and can retry at 1×.
- [x] Preserve process-local private queue state across temporary authority/database failures while suspending output and rejecting unverified commands. Revalidate before recovery; clear on definitive session rejection.
- [ ] Build Tauri clients for macOS, Windows, and Linux with bundled local backend lifecycle.
- [ ] Add Symphonia decoding, CPAL output, FFmpeg fallback, and operating-system media controls.
- [ ] Fix current-track metadata in the system Now Playing panel. The reported panel shows only "Hito / Now Playing" and a placeholder instead of the playing track. Publish the authorized title, artist, album and artwork when available, plus duration, position and playback state through browser and native media integrations. Expose supported playback controls and verify track changes, seeking, pause/resume, queue completion and account switching without stale or private metadata remaining after logout.
- [ ] Add verified HTTP sources, preference ranking, manual override, server relay, direct fallback, conversion, and bitrate settings.
- [ ] Complete failover near the current position, deferred-source handling, gapless playback, and codec compatibility fixtures.
- [ ] Prepare the next queue entry in the background while the current track plays. Check local files, saved copies and verified ready sources first; otherwise search approved providers, resolve a suitable match and start source preparation early. Buffer playable audio before the transition to avoid waiting at track end. Bound lookahead, cancel stale work after queue/account/source-choice changes, preserve current playback and manual choices, and keep temporary buffering separate from explicit downloads. Verify browser/native transitions with shuffle, repeat, skips and slow providers.
- [ ] Verify local audio opens, metadata, and ongoing reads on stalled or disconnected drives.
- [ ] Verify metadata compatibility, including a bounded policy for compressed ID3 tags, which are currently unsupported.
- [ ] Complete queue reordering and playback across catalog pages; verify very large queues against the performance gates.
- [ ] Verify that desktop works without a server and web explains unavailable connections.

## Synchronization and downloads

- [ ] Build a durable local replica and offline operation queue, reconnect handling, and account isolation.
- [ ] Add previewed standalone-library import while preserving its separate database.
- [ ] Verify concurrent edits, duplicate delivery, deletions, permissions, and device-local availability across replicas.
- [x] Deploy explicit provider-track downloads with durable intents, transfer progress, verification, restart/provider-outage playback and intent-specific removal. Verify the release-image browser workflow and migration preservation. See [download evidence and limits](docs/persistent-downloads.md).
- [x] Implement and deploy playlist downloads locally with private retention roots, shared copies, Follow reconciliation, durable explicit retries and a cancellable web workflow. Verify duplicate entries, membership changes, temporary source loss, cleanup and original/private-state preservation. See [development evidence and remaining checks](docs/playlist-downloads.md).
- [x] Verify the playlist-download release image in disposable Chromium, including duplicate entries, committed-response retry, provider-outage playback, removal and original/private-state preservation.
- [x] Rehearse playlist-download migration on a restored whole-volume clone, verify stopped preservation and runtime separately, then back up and upgrade the normal local server.
- [ ] Define and implement Download behavior for existing user-owned local originals.
- [ ] Keep followed downloads synchronized with reference counting; preserve originals and library records.
- [ ] Separate temporary playback cache from persistent downloads; handle disk exhaustion and interrupted transfers.
- [ ] Add configurable automatic storage cleanup for all Hito-managed media, including music, podcast episodes, audiobooks and associated media assets. Rank eligible copies by playback frequency and recency within a storage budget, reclaim less-used media and retrieve it again on demand. Expose a visible feature flag users can disable. Reclaim only cache and automatically saved, unpinned copies; always retain media explicitly selected with Download / Keep offline and user-owned originals. Preserve library records, metadata, progress and shared retention references. Define the default setting before implementation; verify disk pressure, unavailable sources, offline playback and shared copies.

## Providers and content

- [x] Implement the versioned HTTP manifest contract and private installation lifecycle.
- [x] Add core provider search and durable adoption with owner-only libraries, stable identities, and provider provenance.
- [x] Add authenticated provider search/adoption APIs and bounded private audio relay with byte ranges and live cancellation.
- [x] Add browser discovery, explicit save/adoption, and playback through authorized server-relayed provider sources.
- [x] Add source-only provider refresh, bounded browser failover near the requested position, and explicit track-details availability checks.
- [x] Review and attach alternate provider audio to an existing recording without replacing its metadata or playlist identities. Verify the complete copied-track workflow and recovery through actual audio in Chromium. Deployed locally after backup, clone restoration and migration checks; see [source attachment](docs/provider-sources.md).
- [ ] Automatically find and select suitable audio for imported playlists through approved connected providers, including Spotify metadata with YouTube audio. Keep playback usable without per-track source confirmation, preserve recording versions and private identities, bound background work, and provide a durable manual source override that automatic retries and Follow synchronization cannot replace. Verify actual playback and the absence of implicit persistent downloads.
  - [x] Implement bounded core/browser resolution, inherited musical metadata and approved provider evidence in the development branch.
  - [x] Implement durable manual source choice, current-state reads, return to Automatic and retained-copy origin checks. Verify unread saved-body revocation and dependent-action gating after review findings.
  - [x] Reconcile saved-copy eligibility after source-choice conflicts and refresh available choices after successful attachment without losing drafts.
  - [ ] Verify later concurrent choice revisions and Follow updates preserve the correction and recover current eligibility.
  - [x] Verify the complete image in disposable Chromium with automatic playback, manual correction and distinct saved audio during a provider outage. Live-provider and other platform gates remain separate.
  - [x] Verify one real Spotify-to-YouTube target through ordinary Play, actual AAC playback, seeking and navigation, preserving private state without downloads.
  - [ ] Complete broader live matching, remaining codec paths and native integration.
  - [x] Preserve provider relevance within the three-candidate detail limit and add private structured rejection diagnostics; verify public capability, audio and redaction regressions.
  - [x] Accept and deploy YouTube Music catalog search against the release images, retaining independent metadata hydration and existing matching safeguards. Verify controlled Chromium and real Spotify-to-YouTube WebKit playback.
  - [ ] Carry complete structured artist credits through the plugin contract and primary observations, then verify reordered collaborations and unambiguous feature-title presentation without guessing from display strings.
  - [ ] Investigate intermittent YouTube audio-open HTTP 502 responses. After the music-search update, five opens failed before body delivery while twelve returned 206; a later bounded replay of six recent sources returned 206 throughout. The upstream rejection class remains unconfirmed. User approved deferring this bug while release features continue.
  - [ ] Fix duplicate library cards for the same recording. The reported cards show identical title and artist but different availability. Verify recording/version and provider provenance before merging; preserve playlist occurrences, likes, progress, metadata corrections, manual source choices and playable sources. Keep uncertain matches and distinct performances separate. User requested this fix later.
  - [x] Rehearse migrations 0036–0039 on a stopped whole-volume backup restored into a separate clone; verify preservation and the candidate runtime without account changes.
  - [x] Back up and upgrade the normal server and YouTube service after browser and live-provider acceptance, preserving accounts, credentials and runtime configuration.
- [ ] Complete remaining plugin capability requests/results, conversion, and operational capability negotiation.
  - [x] Add the optional source-persistence declaration with compatible legacy manifest bytes and generated Rust/OpenAPI/TypeScript contracts. Verify playback-only direct admission, terminal playlist status, saturated queues, queued-work withdrawal after approved policy tightening and preservation of prior ready copies after upstream shutdown. Active-body policy changes, mixed-source choices and policy UI acceptance remain open.
- [x] Implement and verify the generic source-preparation backend and plugin contract in development, including durable receipts, restart polling and uncertain-submission guards. The backend is deployed locally after stopped backup/restoration and migration checks. Deferred playback and concrete adapters remain open. See [evidence and limits](docs/source-preparation.md).
- [x] Implement, verify and deploy the shared preparation UI with source labels, current eligibility, exact-request retry, bounded cancellable polling and explicit Play after readiness. Its isolated Chromium workflow passes. See [UI evidence](docs/source-preparation.md#ui-verification).
- [ ] Integrate deferred sources into playback at track boundaries.
- [ ] Ship Real-Debrid, AllDebrid, Premiumize, and TorBox adapters with preparation progress.
- [ ] Ship separately deployed music-folder, Jamendo, and yt-dlp reference plugins. The Jamendo adapter, executable and isolated development container have fixture verification; generic attribution/persistence policy, live integration and broader release acceptance remain open. See [Jamendo development evidence](docs/plugin-jamendo.md).
- [x] Make the first reference plugin runnable in Docker with demo audio and documented UI installation; verify installation, discovery, adoption, playback, and seeking, start the local service, and notify the user here.
- [x] Deploy the folder plugin's private durable recording/release registry, conservative split/merge handling and retained availability to the local development service. Verify its release-image browser workflow, legacy identity/source preservation and restart-stable metadata/revisions.
- [ ] Complete folder-plugin public-release acceptance: active-journal accounting, broader cancellation/crash and codec/upgrade fixtures, full native/GUI, platform and release gates. The service still requires a restart to rescan. See [folder identity and limits](docs/plugin-folder.md).
- [x] Deploy generic playlist Import a copy to the local development server, with atomic private publication, retained/duplicate entries, exact retry and explicit new-copy behavior. Verify three release-image Chromium workflows and selected native device-feature progress/recovery compatibility.
- [ ] Complete playlist-copy public-release acceptance, including official playlist-provider authorization/live checks, full native/GUI and other browser/platform checks. Two independent review rounds completed; final fixes have resolver verification. See [copy/retry ownership and limits](docs/playlist-import.md).
- [x] Deploy public Spotify shared-link import without a developer application or account connection. Verify complete pagination, order, duplicate and unknown entries, rejection of incomplete snapshots and ordinary tracking parameters. Ten owned TCP cases, strict Clippy and a release-image live Chromium import pass. See [setup and limits](docs/plugin-spotify.md).
- [x] Derive an offered plugin service name from its validated manifest so the default setup asks only for its address; retain an optional custom label for protected services. Verify both paths and real folder/Spotify workflows in Chromium, then deploy with backup and preservation checks.
- [x] Retrieve missing recording covers through approved plugins, private durable jobs and shared cancellable UI requests. Verify existing independent copies, duplicate entries, private edits, disconnected-provider retention and account isolation through the release-image browser. Deployed locally after backup, clone restoration and migration checks; see [artwork behavior and limits](docs/plugin-artwork.md).
- [ ] Investigate artwork still missing in the deployed UI, including artist cards and local/imported recordings. User reports persistent placeholders. Verify provider image availability, import/backfill jobs, authorized delivery and rendering end to end; distinguish missing artist/album/playlist support from failed recording-cover retrieval while preserving library identities and personal edits.
- [ ] Import playlist and album artwork through the plugin contract, private Hito storage and authorized UI reads. Add missing artwork to existing independent copies without duplicate playlists, entry changes or lost personal edits.
- [x] Add optional public PKCE manifest declarations, strict validation and legacy wire/digest preservation in the development SDK.
- [ ] Complete and accept session-bound authorization attempts. Local begin/status, encrypted pending secrets and ten-minute/eight-attempt bounds pass the core/server checks, with session/restart/admission/key-recovery and TRACE-redaction fixtures. Migration 0025 and these routes are deployed; remaining revocation, cancellation and expiry boundaries and the complete callback flow stay open. See [authorization groundwork](docs/plugins.md#pending-authorization-attempts-in-development).
- [ ] Add the fixed callback handler, state/expiry/session validation and one-time authorization-code exchange with failure/replay recovery.
- [ ] Add encrypted provider access/refresh-token lifecycle, refresh rotation, expiry handling and connection revocation.
- [ ] Add the shared connection UI and approved desktop browser launcher; verify the complete provider consent flow and account switching.
- [ ] Complete Spotify public-release acceptance: upstream compatibility monitoring, remaining expiry/saturation/cancellation boundaries, branding/linkback and supported browser/platform checks. The public metadata/artwork adapter is deployed locally; optional authorization exchange remains separate. The attribution label is provisional.
- [x] Deploy read-only Follow playlists with durable hourly/daily refresh, stable occurrence reconciliation, bounded workers/history, explicit reviewed connection resume and status/manual refresh UI. Verify provider failures, lost response recovery, live Spotify, stopped backups, restoration and migration preservation. See [Follow behavior and remaining release acceptance](docs/playlist-follow.md).
- [x] Import public Spotify playlists as editable private Hito copies, preserving order, duplicate entries, Spotify identifiers and metadata provenance. Verify a complete live UI import without optional account authorization.
- [x] Follow Spotify playlists with a UI-configured hourly/daily refresh interval and persistent scheduled jobs; reconcile additions, removals, order and duplicates while retaining witnessed entry IDs and the last good snapshot after failed reads. Verify independent editable copies and a live 411-entry public playlist. Download reference integration remains in the downloads checklist.
- [x] Play imported Spotify playlist tracks through the yt-dlp YouTube plugin by explicit source review. The live WebKit case imported 411 entries, attached and played one candidate without replacing identities or private edits, and retained the remaining unavailable entries. The service is deployed locally; automatic playlist-wide association and broader codec/browser acceptance remain open. See [YouTube testing and limits](docs/plugin-youtube.md).
- [ ] Add podcast RSS subscriptions, updates, progress, and downloads.
  - [x] Verify the private core subscription and retained-episode foundation in development, including durable refresh receipts, bounded parsing and archive queries, account isolation and personal-state preservation. See [evidence and remaining work](docs/podcasts.md).
  - [x] Add authorized core RSS audio with stable source IDs, shared stream framing, ranges and unread revocation. Verify exact WAV delivery, preserved state and the 0040-to-0041 backfill across restart. Web/native workflows, downloads and normal-server deployment remain unfinished.
  - [x] Preserve identities and private state across unsubscribe, restart, immutable receipt replay and reactivation. Verify obsolete running-worker retirement and old-grant rejection after reactivation.
  - [x] Expose authenticated RSS JSON routes and join two refresh workers to server shutdown. Verify actual HTTP publication, private audio, account isolation and generated contracts in development. UI, broader active-cancellation checks and normal deployment remain open.
  - [x] Add hourly due checks, exact-address conditional validators, accepted 304 retention and bounded receipt-safe job cleanup. Verify redirect isolation, 36 automatic checks and the additive 0043 migration. Broader recovery and cancellation acceptance remains open.
  - [x] Expose private subscription refresh status, bounded current saved positions and authorized Inertia pages. Verify account privacy, partial reads, retained history and bounded query work at 100,000 recordings.
  - [x] Verify the initial subscription and episode workflow in disposable Chromium and WebKit with owned audio, playback across navigation, persisted progress, reload Resume and explicit Start over. This uses a source-mounted development host; native and release-image acceptance remain separate.
  - [ ] Complete the shared podcast workflow, including broader recovery and native playback/resume. Pages, manual refresh and unsubscribe pass component tests, concurrent-edit recovery and owned-audio Chromium/WebKit checks; normal deployment remains open.
  - [x] Extend durable download origins and authority to RSS episodes. Verify explicit download across logout/core restart/upstream outage, immutable receipts, queued and held-stream unsubscribe, ready-copy retention after unsubscribe/upstream shutdown, and version-43 migration preservation in development. Fresh Chromium/WebKit workflows verify UI Download, retained playback after actual upstream shutdown and explicit removal that preserves the original and personal state. Physical reclamation/shared references, native, packaged-image and deployment acceptance remain open.
- [ ] Add audiobook grouping, chapters, bookmarks, speed, and resume positions.
- [ ] Add timed-lyrics and optional backing-track contracts and rendering.
- [ ] Run reproducible fixture tests and live checks for every official provider.

## UI and branding

- [ ] Regenerate Hito's logo and wordmark with a new visual direction, replace the current orange waveform across the app, and verify readability at small sizes and as an application icon.

## Community service

- [ ] Build and deploy the separate source-ranking/correction API. The development crate has a private SQLite store, closed reporter provisioning, durable playback receipts, candidate-only rankings, generated public `GET /v1/openapi.json` and an executable with private-token-file bootstrap on default loopback port `8095`. Eight community cases pass in Docker, including real-child SIGTERM drain, joined state close, correlated lifecycle logs, TRACE privacy, persisted-reporter restart, private-credential denials, malformed-filter fallback and failed-bind recovery. Container/deployment work, corrections, moderation, rates/quotas, retention, revocation, application opt-ins and live hosting remain open. See [community evidence and limits](docs/community-service.md).
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

The integrated application source at `d483ad5` passed workspace formatting, tests and strict all-target Clippy. Its unchanged frontend has 139 passing cases and verified types/build/contracts. The actual release server passed three playlist-import Chromium workflows, including committed-response retry and explicit new-copy behavior. The focused folder suite passes 38 cases; its actual release-image browser workflow also passes. Eight selected native device-feature progress/recovery cases across six targets passed with an isolated CPAL monitor as UID 10001, plus strict playback Clippy. They are device tests, not fresh GUI or complete native acceptance. Prior totals and overlap are recorded in [current status and verification limits](docs/status.md).

The normal server is healthy on migration 0039 with automatic resolution, manual correction and the shared preparation UI. Controlled Chromium and one real automatic Spotify-to-YouTube WebKit target pass. A stopped whole-volume clone rehearsal and a fresh normal backup precede the upgrade. Both migration audits preserve 91 prior tables, typed rows, rowids, sequences, receipts, keys and host identity; all 39 checksums, integrity, foreign keys and six FTS checks pass. Separate runtime checkpoints change no rows or managed files. Runtime OpenAPI and assets match. The YouTube metadata image is deployed with its runtime policy preserved; Folder and Spotify are unchanged. User capability approval, broader matching, native/platform and public-release acceptance remain separate. See [deployment evidence](docs/status.md#running-local-deployment).

Recording identity fixtures cover ten containers. Query-plan checks on 100,000 duplicate locators avoid temporary sorting. A Catalog API smoke test guards compilation-heavy artist and recording reads with 100,000 synthetic tracks before and after ANALYZE. A fresh-connection SQLx migration test backfills one million recording credits across ten accounts within a deterministic work budget, with and without statistics. These are not release latency benchmarks. [Catalog browsing](docs/catalog-browsing.md) documents exact credits, source appearances, Unicode search, and paged playback limits. An isolated production container exits successfully on Docker stop with lifecycle logs. Packaged backup restoration on each supported platform and the remaining public-release gates remain unfinished.
