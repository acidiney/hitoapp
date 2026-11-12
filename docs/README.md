# Hito documentation

Start with the [README](../README.md) if you haven't run Hito yet. Hito is in development; these pages describe the current build, and each one says what is still missing. The [roadmap](roadmap.md) shows what works today and what comes next.

## Run Hito

- [Self-hosting with addons](self-hosting.md): start, connect, update and remove addons.
- [Music folders](music-folders.md): share folders, scan them and keep them read-only.
- [Operations](operations.md): backups, restore, upgrades and shutdown.
- [Install as a web app](web-app-installation.md) on a phone or desktop browser.
- [Account security](account-security.md) and the [screen lock](screen-lock.md).

## Use Hito

| Topic | Pages |
| --- | --- |
| Library | [Albums, artists and genres](catalog-browsing.md) · [Editing track details](metadata.md) · [Metadata providers](metadata-preferences.md) · [Listening history](listening-history.md) |
| Artists | [Matching](artist-matching.md) · [Reviewing a match](artist-review.md) · [Identity groups](artist-identity-groups.md) · [Discography](artist-discography.md) · [Similar artists](artist-similarity.md) |
| Playlists | [Import](playlist-import.md) · [Follow](playlist-follow.md) · [Metadata refresh](playlist-metadata-refresh.md) · [Downloads](playlist-downloads.md) |
| Playback | [Queue](queue.md) · [Choosing a source](source-preference.md) · [Automatic sources](automatic-sources.md) · [Manual sources](manual-sources.md) · [Downloads](persistent-downloads.md) |
| More | [Podcasts](podcasts.md) · [Audiobooks](audiobooks.md) · [Desktop app](desktop.md) |

## Addons

- [How plugins connect](plugins.md): the protocol, review flow and credentials.
- Reference addons: [Spotify](plugin-spotify.md) · [YouTube](plugin-youtube.md) · [Jamendo](plugin-jamendo.md) · [LRCLIB](plugin-lrclib.md) · [Folder](plugin-folder.md) · [Debrid services](debrid-adapters.md)
- Optional capabilities: [catalog](plugin-catalog.md) · [lyrics and backing tracks](plugin-timed-content.md) · [artwork](plugin-artwork.md)

## Build and contribute

- [Development](development.md): Docker setup, Makefile targets, worktrees and disk budget.
- [Testing](testing.md): test layers and how to run one test.
- [Architecture](architecture.md) and [architecture decisions](architecture-decisions.md).
- [Design proposals (RFCs)](rfcs/README.md).
- [API](api.md) and the generated [OpenAPI schema](openapi.json).
- [Logging](logging.md): what is logged and what never is.
- [Design system](design-system.md): tokens and components of the web app.

<details>
<summary>Internals</summary>

- Identity and access: [recording identity](recording-identity.md) · [playback access](playback-access.md) · [playback leases](playback-leases.md) · [artist catalog host](artist-catalog-host.md) · [recording attribution](recording-attribution.md) · [built-in MusicBrainz](builtin-musicbrainz.md)
- Playback engine: [native playback](native-playback.md) · [native coordinator](native-coordinator.md) · [native audio](native-audio.md) · [decoding fallback](native-decoding-fallback.md) · [next-entry preparation](next-entry-preparation.md) · [browser media session](browser-media-session.md) · [audio conversion](audio-conversion.md) · [latency measurement](performance.md)
- Sources and storage: [provider sources](provider-sources.md) · [source preparation](source-preparation.md) · [automatic storage cleanup](automatic-storage.md) · [offline replicas](offline-replicas.md) · [database maintenance](database-maintenance.md)
- Sync and community: [catalog replication](catalog-replication.md) · [catalog journal design](catalog-journal-design.md) · [community service](community-service.md)

</details>

## Legal

[Third-party notices](third-party-notices.md) · [SQLx SQLite provenance](third-party/sqlx-sqlite.md) · [licenses](licenses/)
