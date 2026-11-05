# Documentation index

This page lists every document in `docs/` once, by topic. For the repository layout, see the [root README](../README.md). Each document says whether it describes current behavior, tested evidence, planned work or an external dependency.

Everything here describes a local development state. No document describes a public release. The local development server ran migrations through 0081 on 2026-11-02.

## Scope and status

- [Release plan](release-plan.md): the full requested scope and the public release gates.
- [Implementation status](status.md): dated evidence and unfinished work. The newest summary is at the top.
- [TODO checklist](../TODO.md): open and finished work items, each linked to its evidence.

## Architecture and design

- [Architecture](architecture.md): crate graph, runtime topology, background workers (with the polling loop diagram) and their external calls, core composition, plugin protocol, source resolution, account deletion, data ownership.
- [Architecture decisions](architecture-decisions.md): short records of the decisions behind capability services.
- [Design proposals (RFCs)](rfcs/README.md): the index of four proposals and their status.
  - [RFC 0001, provider source lifecycle](rfcs/0001-provider-source-lifecycle.md): proposed.
  - [RFC 0002, playback access boundary](rfcs/0002-playback-access-boundary.md): implemented.
  - [RFC 0003, page read models](rfcs/0003-page-read-models.md): implemented.
  - [RFC 0004, host and sync client identity](rfcs/0004-host-and-sync-client-identity.md): proposed.
- [Recording identity](recording-identity.md): stable recording IDs, sources, and why a tag edit or moved file does not change identity.
- [Playback access](playback-access.md): how core grants audio for one exact recording and source.
- [API](api.md) and the generated [OpenAPI schema](openapi.json): the `/api/v1` contract.
- [Logging](logging.md): what workflows log and what they never log.
- [Design system](design-system.md): tokens, components and the screens of the web redesign. The redesign covers every screen the server can fill. Open items are listed there.

## Development and testing

- [Development](development.md): Makefile, Docker rules, worktrees, disk budget, running one test.
- [Testing](testing.md): test layers, grouped binaries, fake plugins, flake policy, load-sensitive tests and the known flaky `indexing_snapshot`.
- [Third-party notices](third-party-notices.md) and [sqlx-sqlite notes](third-party/sqlx-sqlite.md): licenses and the vendored SQLite driver.

## Library, catalog and metadata

- [Music folders](music-folders.md): folder grants, scans, watching, desktop folder choice.
- [Catalog browsing](catalog-browsing.md): albums, artists, genres, artist radio, complete track lists.
- [Metadata](metadata.md): personal title and artist edits, source observations, tag identifiers.
- [Metadata preferences](metadata-preferences.md): provider order and the three automatic lookup choices in Settings, Privacy.
- [Built-in MusicBrainz](builtin-musicbrainz.md): the fixed-origin client for artist search, profiles, discographies and genres.
- [Recording attribution](recording-attribution.md): optional attribution on provider metadata.
- [Artist matching](artist-matching.md): file tag, ISRC, provider link and name matching, the background job and its consent rules.
- [Artist catalog host](artist-catalog-host.md): reviewed associations, unlinking, splitting, shared corrections, deletion and retention.
- [Artist review](artist-review.md): searching and confirming an artist for one credit.
- [Artist identity groups](artist-identity-groups.md): chosen artists, group pages and bulk choice.
- [Artist discography](artist-discography.md): release groups, editions, types and library availability.
- [Similar artists](artist-similarity.md): the optional section and the ListenBrainz plugin evaluation.
- [Listening history](listening-history.md): the per-account pause setting, deleting history, what History and Home read, and what is kept.
- [Database maintenance](database-maintenance.md): the migration-only command, backups and per-migration notes up to 0081, with links to the feature documents.

## Playback and sources

- [Playback queue](queue.md): queue entries and ownership in the browser and the desktop app.
- [Native playback](native-playback.md), [native coordinator](native-coordinator.md) and [native audio engine](native-audio.md): the Rust player, its command owner and the decoder and output.
- [Browser media session](browser-media-session.md): Media Session metadata, duration revisions and source recovery.
- [Next-entry preparation](next-entry-preparation.md): preparing the upcoming queue entry.
- [Automatic sources](automatic-sources.md): finding audio for a saved recording.
- [Manual sources](manual-sources.md): choosing a source by hand.
- [Provider sources](provider-sources.md): several providers for one recording.
- [Source preparation](source-preparation.md): the backend and UI for sources that need preparation.
- [Persistent downloads](persistent-downloads.md), [playlist downloads](playlist-downloads.md) and [automatic storage cleanup](automatic-storage.md): downloads (cleanup is planned only).

## Playlists, audiobooks and podcasts

- [Playlist import](playlist-import.md), [playlist follow](playlist-follow.md) and [playlist metadata refresh](playlist-metadata-refresh.md).
- [Audiobooks](audiobooks.md): grouping, bookmarks and Continue.
- [Podcasts](podcasts.md): RSS subscriptions, episodes and downloads.
- [Screen lock and optional PIN](screen-lock.md): per-account session lock.

## Plugins and providers

- [Plugin connections and protocol](plugins.md): protocol, review flow, credentials, curated catalog admission.
- [Catalog extension contracts](plugin-catalog.md), [timed lyrics and backing tracks](plugin-timed-content.md) and [provider artwork](plugin-artwork.md): optional plugin capabilities, plus the cover lookup flow, Cover Art Archive covers and artist photos.
- [Folder plugin](plugin-folder.md), [Spotify plugin](plugin-spotify.md), [YouTube plugin](plugin-youtube.md) and [Jamendo plugin](plugin-jamendo.md).
- [Debrid adapters](debrid-adapters.md): Real-Debrid, All-Debrid, TorBox and Premiumize.

Some crates and plugins also have their own README: `crates/remote-audio`, `crates/debrid/*` (Premiumize, All-Debrid, TorBox) and `plugins/premiumize`, `plugins/torbox`.

## Apps and community

- [Desktop host](desktop.md): the Tauri host and its native player.
- [Community service](community-service.md): the separate opt-in reporting and ranking service.

## Operations

- [Operations](operations.md): Docker deployment and management of the local server.
- [VPS deployment](vps-deployment.md) and the [VPS runbook](../deploy/vps/README.md): a development deployment, not a public release.
