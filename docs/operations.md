# Docker operations

The normal server is `hito-server-1`. Its health endpoint checks SQLite. Startup applies versioned SQLx migrations before accepting requests. The process exits if the database or built UI cannot be opened.

```sh
docker compose ps
docker compose logs --tail=100 server
curl --fail http://localhost:8080/health
```

## Storage

```mermaid
flowchart LR
  HostMusic[Host music folder] -->|read-only bind mount| Music[/music]
  Volume[Docker hito-data volume] -->|local persistent mount| Data[/data/hito.db]
  Server[Server process UID 10001] -->|reads| Music
  Server -->|SQLite WAL transactions| Data
```

Development setup launchers configure read-only mounts through native folder selection. Administrator Settings connects folders inside those grants. See [music folders](music-folders.md) for setup, removal, and verification limits. Scanning never modifies original audio. Private inspection copies are removed after use. Only a completed folder walk reconciles missing files.

For a guided installation, use its generated Compose file, for example `docker compose -p hito -f "$HOME/Library/Application Support/Hito/compose.json" ps` on macOS. Substitute the saved project and platform configuration directory. The repository Compose file remains the advanced development setup.

Tag-only changes preserve recording and source identities when verified audio evidence matches. Actual replaced audio receives a different recording and source, preserving references to the old recording. [Recording identity](recording-identity.md) explains conservative legacy matching and unavailable historical records.

Scanning is a persistent job. Workers claim queued jobs with a lease and renew it during enumeration, verification, and reconciliation. A restarted worker can reclaim an expired lease. Failed jobs report a generic actionable message through the API. Scans reread file checksums and reuse completed unchanged inspections; incremental watching remains unfinished. FFmpeg and FFprobe are required and included in the Docker images. `HITO_FFMPEG` and `HITO_FFPROBE` can override their executable paths in a binary deployment. Raw media-tool diagnostics are discarded.

The backend persists its host identity in `hito.device-id` beside `hito.db`. Preserve this file with the database when backing up or moving the service. An existing prototype database without a sidecar adopts its legacy `server` identity so its audio stays available. Fresh installations generate a UUID. `HITO_DEVICE_ID` explicitly overrides the saved identity without rewriting it. Synchronization client IDs are separate and do not grant access to local files.

## Restart and upgrade

```sh
docker compose restart server
docker compose up --build -d server
```

The data volume survives both commands. Do not run `docker compose down -v` against a library you want to keep. Desktop update approval, signed packages, and automated rollback verification are still release work.

Unix binaries handle SIGTERM and SIGINT; Windows uses Ctrl+C. Shutdown stops accepting connections, allows five seconds for HTTP responses to drain, then cancels and joins remaining connections. The core finishes transaction boundaries, requeues interrupted scans, and reaps inspection subprocesses before the host finishes. Public regressions cover active inspection, pending HTTP handlers and bodies, and restart ownership. Disconnected-drive behavior and full recovery acceptance remain release gates.

## Backups

Preserve `hito.plugin-key` beside the database. It decrypts personal plugin credentials; a database-only backup cannot recover those tokens. A missing or invalid key leaves local playback enabled but disables credential-dependent plugin checks. Restore the matching private key and restart. See [plugin recovery](plugins.md#credential-key-and-backups).

Until the online backup interface is implemented, stop the normal server and archive the complete data volume. Copying only `hito.db` while SQLite is running can miss committed WAL data.

```sh
docker compose stop server
mkdir -p backups
chmod 700 backups
docker run --rm -v hito_hito-data:/data:ro -v "$PWD/backups:/backup" debian:bookworm-slim sh -c 'umask 077; tar -C /data -czf /backup/hito-data.tar.gz .'
docker compose start server
```

To restore, stop the server, extract into a new empty Docker volume, and start a separate Hito instance against that volume. Keep the original volume until the restored library has been checked. A stopped local whole-volume backup was restored and migrated in an isolated container with byte, permission, ownership and database checks. See [deployment evidence](status.md#running-local-deployment). Repeatable packaged backup/restore acceptance on every supported platform remains a release gate.

## Network and sessions

The default listener is exposed only on `127.0.0.1`. A public installation needs an HTTPS reverse proxy and an exact `HITO_PUBLIC_ORIGIN` matching the browser URL. Set that environment variable in Compose when changing the origin. HTTPS makes session cookies Secure; cookies are always HttpOnly and SameSite Strict.

For access over a local network, publish the server port on the host's LAN address and explicitly list any other browser origins in `HITO_ADDITIONAL_ORIGINS`. For example, a primary origin of `http://192.168.1.50:8080` can also accept `http://localhost:8080,http://127.0.0.1:8080`. The list permits at most 16 comma-separated, canonical HTTP(S) origins, with no paths, trailing slash, credentials or wildcards. The primary origin still controls cookies and authorization callbacks. An HTTPS primary requires HTTPS aliases so its Secure cookies remain usable.

Browsers with Web Crypto retain encrypted Inertia history. On local HTTP without `crypto.subtle`, browser history contains only a navigation marker and bounded scroll coordinates. Back and Forward fetch an authenticated page again while keeping the player mounted. Private page and form data are never written to unencrypted history. UUID creation uses cryptographic `getRandomValues` when `randomUUID` is unavailable.

Cookie-authenticated mutations send `X-Hito-Request: 1`. API clients can use bearer tokens. Login/setup have per-client rate limits, bounded request bodies, a body-read timeout, and limited password-hashing concurrency. Do not trust arbitrary forwarded IP headers; the server uses its actual connection peer. Behind a reverse proxy, shared peer limits apply until trusted-proxy handling is implemented.

Logs omit session tokens and request bodies. Compose enables structured JSON with bounded rotation. [Logging](logging.md) documents filters, browser logs, correlation, and redaction tests. Plugin installation and credential workflows have dedicated trace-redaction checks. Future capability operations and official provider adapters require their own verification before release.
