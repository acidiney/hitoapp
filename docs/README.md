# Documentation index

This page lists every document in `docs/` by topic. Files stay where they are, so links from other branches keep working. For the repository layout, see the [root README](../README.md).

Each document says whether it describes current behavior, tested evidence, planned work or an external dependency. [Status](status.md) holds the dated evidence. [Release plan](release-plan.md) holds the scope.

## Architecture and design

- [Architecture](architecture.md): crate graph, runtime topology, plugin protocol, source resolution, data ownership.
- [Architecture decisions](architecture-decisions.md)
- [Recording identity](recording-identity.md)
- [Playback access](playback-access.md)
- [API](api.md) and the generated [OpenAPI schema](openapi.json)
- [Logging](logging.md)
- [Design proposals (RFCs)](rfcs/README.md): proposed architecture changes, not yet built.

## Development and testing

- [Development](development.md): Makefile, Docker rules, worktrees, disk budget, single tests.
- [Testing](testing.md): test layers, helpers, flake policy.
- [Third-party notices](third-party-notices.md) and [sqlx-sqlite notes](third-party/sqlx-sqlite.md)

## Library, catalog and metadata

- [Music folders](music-folders.md)
- [Catalog browsing](catalog-browsing.md)
- [Metadata](metadata.md), [metadata preferences](metadata-preferences.md), [built-in MusicBrainz](builtin-musicbrainz.md)
- [Recording attribution](recording-attribution.md)
- Artists: [catalog host](artist-catalog-host.md), [discography](artist-discography.md), [identity groups](artist-identity-groups.md), [review](artist-review.md)
- [Database maintenance](database-maintenance.md)

## Playback and sources

- [Native playback](native-playback.md), [native coordinator](native-coordinator.md), [native audio engine](native-audio.md)
- [Browser media session](browser-media-session.md)
- [Next-entry preparation](next-entry-preparation.md)
- [Automatic sources](automatic-sources.md), [manual sources](manual-sources.md), [provider sources](provider-sources.md), [source preparation](source-preparation.md)
- [Persistent downloads](persistent-downloads.md), [playlist downloads](playlist-downloads.md), [automatic storage cleanup](automatic-storage.md)

## Playlists, audiobooks and podcasts

- [Playlist import](playlist-import.md), [playlist follow](playlist-follow.md), [playlist metadata refresh](playlist-metadata-refresh.md)
- [Audiobooks](audiobooks.md)
- [Podcasts](podcasts.md)

## Plugins and providers

- [Plugin connections and protocol](plugins.md), including curated catalog admission
- [Catalog extension contracts](plugin-catalog.md), [timed lyrics and backing tracks](plugin-timed-content.md), [provider artwork](plugin-artwork.md)
- [Folder plugin](plugin-folder.md), [Spotify plugin](plugin-spotify.md), [YouTube plugin](plugin-youtube.md), [Jamendo plugin](plugin-jamendo.md)
- [Debrid adapters](debrid-adapters.md): Real-Debrid, All-Debrid, TorBox, Premiumize.

Some crates and plugins also have their own README: `crates/remote-audio`, `crates/debrid/*` (Premiumize, All-Debrid, TorBox) and `plugins/premiumize`, `plugins/torbox`.

## Apps and community

- [Desktop host](desktop.md)
- [Community service](community-service.md)

## Operations

- [Operations](operations.md): Docker deployment and management.
- [VPS deployment](vps-deployment.md) and the [VPS runbook](../deploy/vps/README.md): a development deployment, not a public release.

## Status and scope

- [Release plan](release-plan.md)
- [Implementation status](status.md)
- [TODO checklist](../TODO.md)
