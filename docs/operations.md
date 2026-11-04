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

Scanning is a persistent job. Workers claim queued jobs with a lease and renew it during enumeration, verification, and reconciliation. A restarted worker can reclaim an expired lease. Failed jobs report a generic actionable message through the API. Scans reread file checksums and reuse completed unchanged inspections; changed files in connected server folders queue an incremental rescan automatically (see below). FFmpeg and FFprobe are required and included in the Docker images. `HITO_FFMPEG` and `HITO_FFPROBE` can override their executable paths in a binary deployment. Raw media-tool diagnostics are discarded.

The backend persists its host identity in `hito.device-id` beside `hito.db`. Preserve this file with the database when backing up or moving the service. An existing prototype database without a sidecar adopts its legacy `server` identity so its audio stays available. Fresh installations generate a UUID. `HITO_DEVICE_ID` explicitly overrides the saved identity without rewriting it. Synchronization client IDs are separate and do not grant access to local files.

### Music scanning

Hito watches connected server music folders and rescans a folder shortly after its audio files change. `HITO_FOLDER_WATCH` selects `auto` (default: native events, with polling where native events are unavailable or unreliable), `poll` (always poll) or `off`; values are case-insensitive and an empty value means the default. `HITO_FOLDER_WATCH_POLL_SECONDS` sets the polling interval, from 5 to 86400 seconds (default 30, ignored when watching is off). Invalid values stop startup. Changes are grouped for two seconds of quiet before a scan is queued. A scan after a change rereads checksums, so watching a large library that changes often costs disk reads.

Native watching keeps one kernel watch per directory, which counts against the host's `fs.inotify.max_user_watches` and container memory. Polling walks the whole folder each interval and slows itself down on large folders. Network shares, removable drives and Docker Desktop bind mounts often miss native events, so Hito polls them. Raise the host limit if the log reports `watch_limit_reached`. Log events are `folder_watching_started`, `folder_watch_started`, `folder_watch_polling` (with a `reason`), `folder_watch_scan_retry` and `folder_watch_stopped`; frequent request events are debug level. Logs never contain folder paths. When the server starts, each folder is compared cheaply with its last scan and rescanned only if something changed.

## Compose files

The documented command for the normal server stays `docker compose` with the root `compose.yaml`, project `hito`, container `hito-server-1` and volume `hito_hito-data`. Plugin overlays do not change that service. They live in `deploy/compose/<plugin>.yaml`, extend `deploy/compose/plugin-base.yaml` for the hardened runtime (read-only root, all capabilities dropped, `no-new-privileges`, process and memory limits) and build `docker/plugin.Dockerfile`. Run one beside the root file:

```sh
docker compose -f compose.yaml -f deploy/compose/folder.yaml up -d plugin-folder
```

Check an overlay without starting anything:

```sh
docker compose -p hito -f compose.yaml -f deploy/compose/spotify.yaml --profile plugins config --quiet
```

The development VPS has its own build and runtime files in `deploy/vps`. [Development](development.md#docker-only) has the layout diagram and the plugin image build arguments.

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

## When the server cannot be reached

Current behavior. The browser client notices four kinds of trouble: the connection fails (server stopped, restarting or off the network), the server does not answer within 90 seconds, a proxy answers 502, 503 or 504 with its own page instead of a Hito error (a 5xx that carries a Hito error message is not an outage), and the same during a page visit. Plugin install and preview, source preparation requests and removing missing recordings wait up to 10 minutes before they count as timed out. It then shows one banner that says "Cannot reach Hito", what to check and that it is reconnecting. It never shows status codes, addresses, request paths or proxy page text. Errors that Hito itself sends, such as a provider failing behind a 502 with a Hito message, are not treated as an outage and keep their own wording.

The page, scroll position, typed text and the playing queue stay where they were. The client asks `/health` after 2, 4, 8 and then every 15 seconds, also when the browser reports it is online again or the tab becomes visible, and has a "Try now" button. When the server answers, the banner disappears by itself. If a page visit failed during the outage, the current page reloads its data in place, at most once every 15 seconds. A form or other change submitted by a page visit that failed is never sent again automatically. The page says "Your last change was not saved" until dismissed. Signing in, out or switching account clears any outage state. If the server is already down when the page first opens, the page keeps retrying and reloads itself when the server returns, at most once every 15 seconds so that a server that answers `/health` but still fails the session check cannot cause a reload loop. A skipped reload is retried after the remaining wait. The limit survives a reload through session storage, or the window name when storage is blocked, and is cleared once a page loads. "Try now" there reloads at once. Only the page and background data stop updating while it is down. Audio already buffered by the browser can keep playing, but starting a new track needs the server.

Evidence. 34 new web cases cover each failure class, the backoff, recovery, the plain wording and that no raw text leaks. A Chromium test aborts the isolated server's requests mid-session, then answers with a proxy 502 page, and checks the banner, the unchanged page and typed text, and automatic and manual recovery. Limits: a request that stays open after the server has sent its headers is not timed out, a 503 with an unreadable body is only treated as an outage when the caller reads error bodies, and the test simulates the outage in the browser rather than stopping the server container. Native desktop playback uses the same client against its private local backend, so a backend failure there looks the same.

## Metadata contact

`HITO_METADATA_CONTACT` is optional. When set, it is added to the User-Agent of the requests the server makes to MusicBrainz, the Cover Art Archive, Wikidata and Wikimedia Commons, as `Hito/<version> (<contact>)`, so those services can reach the operator. Use an address or page you are willing to publish to them. It must be 1 to 128 visible ASCII characters with no spaces or parentheses; any other value stops the server from starting. Unset, the User-Agent is `Hito/<version>`. The value is never logged or sent elsewhere.
