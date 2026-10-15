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

Changing the host music folder currently requires recreating the container. Guided host folder selection and multiple-folder Settings are approved but unfinished. Scanning does not modify or remove original audio. It creates private temporary inspection copies and removes them after use. Missing files and sources from a displaced configured root become unavailable after a successful scan.

Tag-only changes preserve recording and source identities when verified audio evidence matches. Actual replaced audio receives a different recording and source, preserving references to the old recording. [Recording identity](recording-identity.md) explains conservative legacy matching and unavailable historical records.

Scanning is a persistent job. Workers claim queued jobs with a lease and renew it during enumeration, verification, and reconciliation. A restarted worker can reclaim an expired lease. Failed jobs report a generic actionable message through the API. Scans reread file checksums and reuse completed unchanged inspections; incremental watching remains unfinished. FFmpeg and FFprobe are required and included in the Docker images. `HITO_FFMPEG` and `HITO_FFPROBE` can override their executable paths in a binary deployment. Raw media-tool diagnostics are discarded.

The backend persists its host identity in `hito.device-id` beside `hito.db`. Preserve this file with the database when backing up or moving the service. An existing prototype database without a sidecar adopts its legacy `server` identity so its audio stays available. Fresh installations generate a UUID. `HITO_DEVICE_ID` explicitly overrides the saved identity without rewriting it. Synchronization client IDs are separate and do not grant access to local files.

## Restart and upgrade

```sh
docker compose restart server
docker compose up --build -d server
```

The data volume survives both commands. Do not run `docker compose down -v` against a library you want to keep. Desktop update approval, signed packages, and automated rollback verification are still release work.

Unix binaries handle SIGTERM and SIGINT; Windows uses Ctrl+C. The server and indexing worker log shutdown lifecycle events. CLI tests and an isolated production container verify clean idle shutdown. Bounded shutdown during active long scans and streams remains part of the recovery release gates.

## Backups

Until the online backup interface is implemented, stop the normal server and archive the complete data volume. Copying only `hito.db` while SQLite is running can miss committed WAL data.

```sh
docker compose stop server
mkdir -p backups
chmod 700 backups
docker run --rm -v hito_hito-data:/data:ro -v "$PWD/backups:/backup" debian:bookworm-slim sh -c 'umask 077; tar -C /data -czf /backup/hito-data.tar.gz .'
docker compose start server
```

To restore, stop the server, extract into a new empty Docker volume, and start a separate Hito instance against that volume. Keep the original volume until the restored library has been checked. Automated backup/restore acceptance tests remain a release gate.

## Network and sessions

The default listener is exposed only on `127.0.0.1`. A public installation needs an HTTPS reverse proxy and an exact `HITO_PUBLIC_ORIGIN` matching the browser URL. Set that environment variable in Compose when changing the origin. HTTPS makes session cookies Secure; cookies are always HttpOnly and SameSite Strict. Inertia encrypted history requires HTTPS or localhost.

Cookie-authenticated mutations send `X-Hito-Request: 1`. API clients can use bearer tokens. Login/setup have per-client rate limits, bounded request bodies, a body-read timeout, and limited password-hashing concurrency. Do not trust arbitrary forwarded IP headers; the server uses its actual connection peer. Behind a reverse proxy, shared peer limits apply until trusted-proxy handling is implemented.

Logs omit session tokens and request bodies. Compose enables structured JSON with bounded rotation. [Logging](logging.md) documents filters, browser logs, correlation, and redaction tests. Provider credential storage and future integrations require their own redaction verification before release.
