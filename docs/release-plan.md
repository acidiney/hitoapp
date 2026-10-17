# First public release

This file preserves the requested scope. A working development deployment does not satisfy the public release gates. See [status.md](status.md) for evidence and unfinished work.

## Architecture

- Rust monorepo for core, Axum/Tokio server, SQLx/SQLite migrations and FTS, persistent jobs, synchronization, native playback, adapters, reference services, and community API.
- Shared React/TypeScript UI through a thin adapter for the [Inertia protocol](https://inertiajs.com/the-protocol).
- Tauri desktop on macOS, Windows, and Linux. A bundled local backend owns the standalone library; remote mode synchronizes rather than loading a remote webpage. Mobile follows later.
- Native decoding with Symphonia, CPAL output, and FFmpeg fallback. Browser audio uses the same command and event model.
- Versioned JSON APIs, OpenAPI, language-neutral HTTP plugins. Manifest capabilities cover search, catalogs, metadata, sources, playlists, dashboard sections, timed lyrics, and backing tracks. Hito renders structured data and never executes downloaded plugins.
- Separate recordings, releases, sources, and device-local files. Preserve provider IDs and metadata provenance.
- AGPL-3.0-only application; Apache-2.0 plugin SDK.

## Library, identity, and sync

- Automatic standard Music-folder scanning on desktop; additional folders and drives.
- Read tags before optional enrichment. MusicBrainz is the built-in default metadata provider, ready to use without a separate plugin deployment. Users can replace it with compatible HTTP metadata plugins or disable enrichment. The built-in provider and external plugins use shared capability contracts; Hito owns rendering and preserves provenance and personal overrides.
- Standalone SQLite; server-authoritative remote mode with local cache and durable offline operations.
- Stable playlist-entry IDs, idempotency, independent offline merges, deletion wins, conflicting fields ordered by server acceptance.
- Device-only files available on their owning device. Other devices need another source.
- Preview standalone playlists, likes, and progress before import into a server; preserve the standalone library.
- Independent accounts, private activity/credentials, admin-managed shared library access, admin-created accounts by default, optional local PIN locks.
- Personal plugins and shared admin provider defaults. Personal metadata edits, administrator promotion, unlinking, splitting, and correction undo.

## Listening

- Search, artists, albums, playlists, likes, history, editable queues, shuffle, repeat, seek, gapless playback, explicit offline downloads.
- RSS podcasts with updates, subscriptions, progress, and downloads. Audiobooks with grouping, chapters, bookmarks, speed, and resume positions.
- Device-independent sessions and queues. Remote control, handoff, and group playback deferred.
- Merge verified matches; covers, live performances, and remixes stay separate. Uncertain matches stay separate.
- Prefer suitable local audio, then ready remote sources ranked by user preference and reliability; allow manual overrides.
- Recover through another verified source of the recording near the current position. Explain failures and continue the queue when recovery fails.
- Prefer original audio, convert for compatibility, offer lower-bitrate streaming. Relay external audio by default; optional direct playback with relay fallback.

## Downloads and integrations

- Persistent downloads start only after Download. Follow playlist changes; reference-count shared copies; preserve originals and library records. Separate temporary cache; never silently evict downloads.
- Import a copy yields an editable playlist. Follow playlist yields a read-only provider-updated playlist.
- Real-Debrid, AllDebrid, Premiumize, TorBox. Prepare unavailable sources automatically, report progress, keep available tracks playing, reconsider deferred tracks at track boundaries.
- Separate official music-folder, Jamendo, and YouTube/yt-dlp plugin services, hosted by users or communities.
- Timed lyrics and backing-track contracts. Microphone mixing, vocal removal, scoring, and recording deferred.

## Community service

- Operated shared source ranking and correction service. Playback works during outages.
- Separate disabled-by-default outcome and correction opt-ins. Rank successful playback, filter to configured providers.
- Exclude audio, filesystem paths, credentials, and signed stream URLs.
- Suggestions first; provider validation or maintainer review before automatic matching. Preserve personal overrides.
- Validation, rate limits, moderation, redacted logs.

## Sequence

1. Foundation and web: workspace, identity, DB, plugins, Inertia, local indexing, browser playback.
2. Desktop: native playback on three platforms, local lifecycle, offline downloads, sync, OS media controls.
3. Integrations: four debrid providers, reference plugins, RSS, audiobooks, playlist import/follow, lyric/karaoke interfaces.
4. Community: opt-in reporting, ranking, corrections, validation, moderation, deployment.
5. Release: compatibility, performance, recovery, packages, signing, backups, installation checks.

## Release gates

- 100,000 tracks, 10 accounts, 5 simultaneous listeners.
- p95 local start ≤250ms, ready remote start ≤2s, available-source recovery ≤3s on fixed documented hardware/network. Measure debrid preparation separately.
- Deterministic fixtures for gapless transitions and navigation; supported codec/conversion matrix, including Symphonia limitations.
- Slow providers, expired links, seek failures, interrupted transfers, unavailable servers, community outages.
- Offline edits, duplicate sync, concurrent playlist changes, deletions, reconnects, account switching.
- Recording deduplication, uncertain matches, correction undo, personal overrides, shared permissions.
- Download synchronization, shared references, disk exhaustion, removal, original-file preservation.
- Reproducible fixtures and live checks for every official provider.
- Signed installers, upgrades, migrations, backup restoration, update approval on macOS, Windows, Linux.

## Distribution and dependencies

One deployable application server with internal workers; plugins and community remain separate deployments. Supply containers, binaries, and Compose; SQLite requires local persistent storage. Updates notify and install only after approval. Signed macOS/Windows installers and Linux packages go to GitHub Releases. App stores are deferred.

Reuse the existing paid Apple Developer account. Apply for SignPath Foundation Windows signing. Public release needs community hosting/DNS, protected signing credentials, provider test accounts, and verified platforms. Local services and unsigned builds support development.

Advanced recommendations, social listening, arbitrary plugin UI, full browser offline operation, mobile, and unrestricted Spotify parity are outside this release.
