# Roadmap

Hito has no public release yet. This page shows what works in the development build, what the first public release (1.0) still needs, and what comes after it.

## Works in the development build

- A self-hosted server with a browser app that installs as a web app on phones and desktops.
- Music folders, read-only, with albums, artists, genres, search and private tag edits.
- The plugin system: an administrator offers plugins, each account connects its own after reviewing their capabilities.
- Reference plugins for Spotify public playlists (import and follow), YouTube, LRCLIB lyrics, Jamendo, ListenBrainz and four debrid services.
- Automatic source finding for imported tracks, a per-account source order, and fallback to the next source when one fails.
- MusicBrainz artist matching, built in, with artist pages and discographies.
- Playlists, likes, history, an editable queue, explicit downloads, podcasts and audiobooks.
- Private accounts, administrator-managed library access and an optional PIN lock.
- A Tauri desktop host with its own library and native Rust playback, in development.

## Before 1.0

**Run anywhere**
- Run the server in Docker or directly on the host.
- Desktop apps for macOS, Windows and Linux that install everything they need, with no Docker.
- Signed installers and Linux packages on GitHub Releases. Updates install only after you approve them.

**Desktop**
- Scan the system Music folder automatically, and add more folders and drives.
- Use a server from the desktop app with a local cache, so playlists, likes and progress keep working offline and sync later.
- Gapless playback, operating-system media controls and offline downloads on all three platforms.

**Playback and sources**
- Recover from a failed source near the current position, explain the failure and keep the queue going.
- Prepare debrid sources automatically, show progress and keep available tracks playing meanwhile.
- Lower-bitrate streaming and format conversion for devices that need it.

**Plugins**
- Plugins for catalogs and home-page sections.
- Lyrics that prefer synced versions, and backing tracks.

**Community service**
- An optional shared service that ranks working sources and collects metadata corrections. Both are separate opt-ins, off by default, and playback never depends on the service being up.

**Quality bar**
- A library of 100,000 tracks with 10 accounts and 5 people listening at once.
- Local tracks start within 250 ms and ready remote tracks within 2 s, measured on fixed, documented hardware.
- Tested recovery from slow providers, expired links, interrupted transfers, offline edits and full disks.
- Tested upgrades, migrations and backup restores on every platform.

## After 1.0

Native mobile apps, remote control and handoff between devices, group listening, karaoke features such as vocal removal, smarter recommendations, and app store releases.

## Not planned

Running plugin code inside Hito, plugins that draw their own UI, full offline use in a plain browser, and matching every Spotify feature.
