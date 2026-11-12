# VPS installation

This configuration targets the existing `forge-alita` Linux amd64 host and the public origin you set in `HITO_PUBLIC_ORIGIN` (for example `https://hito.example.com`). It uses the host's existing Caddy service. Only Hito's server is published, on `127.0.0.1:18080`. Plugin ports are reachable on the separate `hito-vps` Docker network. There is no Docker socket, compiler, browser test service or reverse-proxy container in the runtime configuration.

These files describe an installation. They are not evidence of a completed deployment or public release.

## Build on the development machine

The VPS has limited free disk space and runs images only. Build Linux amd64 images locally through Compose, then transfer them. Existing arm64 images cannot run natively on this host.

After the deployment files are committed, use that clean, committed source tree. Record its full commit and build each service separately, with no other compiler-heavy operation running. The four caches include that commit and architecture so another source tree cannot reuse their Cargo artifacts. The server Dockerfile and the shared `docker/plugin.Dockerfile` use release builds with two Rust jobs, no debug information and no incremental compilation.

```sh
set -eu
export HITO_VPS_REVISION="$(git rev-parse HEAD)"
docker compose -f deploy/vps/compose.build.yaml config --quiet
docker compose --parallel 1 -f deploy/vps/compose.build.yaml build server
docker compose --parallel 1 -f deploy/vps/compose.build.yaml build plugin-folder
docker compose --parallel 1 -f deploy/vps/compose.build.yaml build plugin-spotify
docker compose --parallel 1 -f deploy/vps/compose.build.yaml build plugin-youtube
```

Before and during each build, retain at least 40 GiB of host free space and keep combined Hito Cargo and BuildKit compiler output below 10 GiB. Check the actual output after every image. Stop if the next build cannot fit. Reclaim only identified inactive Hito compiler output after its evidence has been preserved. Do not use a global prune. A build command alone does not verify the resulting amd64 runtime.

For each image, inspect its immutable ID, operating system, architecture and revision label. All must match the intended snapshot before export. For example:

```sh
docker image inspect "hito-vps/server:${HITO_VPS_REVISION}-amd64" \
  --format '{{.Id}} {{.Os}}/{{.Architecture}} {{index .Config.Labels "org.opencontainers.image.revision"}}'
```

Export the four tagged images together so common layers are stored once. Run the following block in Bash; `pipefail` stops a failed image export from being mistaken for a successful archive. Protect the archive from modification and record its SHA-256 before transfer. Keep the archive outside application data and outside Git.

```sh
umask 077
set -o pipefail
docker image save \
  "hito-vps/server:${HITO_VPS_REVISION}-amd64" \
  "hito-vps/plugin-folder:${HITO_VPS_REVISION}-amd64" \
  "hito-vps/plugin-spotify:${HITO_VPS_REVISION}-amd64" \
  "hito-vps/plugin-youtube:${HITO_VPS_REVISION}-amd64" \
  | gzip > hito-vps-images.tar.gz
```

## Prepare the private installation

Use `/srv/hito/config` for Compose, `.env` and `storage.json`; `/srv/hito/music` for original media; and `/srv/hito/backups` for stopped backups. Runtime state has separate named volumes, `hito-vps-data` and `hito-vps-folder-state`. Store another backup off the VPS because these locations share its disk.

Create the host music directory explicitly, even when initially empty. Give UID 10001 read and directory traversal access. Copy `storage.example.json` to `storage.json`, readable by UID 10001, and `.env.example` to `.env` with mode 0600. Set the recorded revision and all four loaded image IDs. The runtime Compose file requires those values and will not pull replacement images or build anything. Keep administrator and provider credentials outside the repository and image archives.

Check the transferred archive hash, available disk space and image sizes before loading. After `docker image load`, inspect all four IDs, platforms and revision labels again. Start from the private configuration directory using explicit paths:

```sh
cd /srv/hito/config
docker compose --env-file .env -f compose.yaml config --quiet
docker compose --env-file .env -f compose.yaml run --rm --no-deps -T --pull never \
  plugin-folder --music /demo --state /state --initialize-state
docker compose --env-file .env -f compose.yaml up -d --no-build --pull never --wait
```

Initialize the folder state exactly once for a new volume. Initialization refuses to overwrite existing state. Normal startup refuses missing or invalid established state. The initial folder plugin supplies three generated test signals from its image, not commercial music or the server's music collection. See [the folder plugin](../../docs/plugin-folder.md) before changing its collection or storage limits.

The server image creates `/data` for UID 10001. Docker initializes its new named volume from that directory. Preserve the complete volume, including `hito.db`, its WAL, `hito.device-id`, `hito.plugin-key` and managed media. Do not replace an existing volume with an empty directory to solve a permission error.

The server has a 3 GiB memory limit and a 1 GiB temporary filesystem for inspection copies. The folder, Spotify and YouTube services have their own limits. Those limits are starting operating bounds, not the 100,000-track release acceptance. Large files or simultaneous scans can exhaust temporary capacity and require an explicit capacity decision. Docker restarts exited services; an unhealthy but running service needs operator diagnosis.

Hito rescans a music folder automatically after its audio files change. The default is native file events (`auto`), which keep one kernel watch per directory, count against the host's inotify limit and the container's memory, and may not see changes on network or removable drives. This deployment sets `HITO_FOLDER_WATCH=poll` with `HITO_FOLDER_WATCH_POLL_SECONDS=300` instead: the music is a read-only host bind mount that may be a separate or removable drive, and polling needs no per-directory kernel state inside the 3 GiB limit. A change appears within about five minutes plus the two-second grouping delay. A poll walks every file's name, size and time without reading contents, and slows itself on large folders. Each scan after a change rereads file checksums. Set `HITO_FOLDER_WATCH=off` to disable it, or `auto` for faster detection on a local disk with enough inotify watches.

## Establish the administrator before public access

Keep the Caddy site unpublished while the operator completes the first administrator setup through a private connection. Send credentials only through the application's authenticated HTTPS setup flow or an operator-controlled local connection. Verify setup is closed and ordinary login works before enabling the public hostname. Do not put credentials in shell arguments, `.env`, logs or this guide.

The host Caddy configuration needs only this site, added without changing existing sites:

```caddyfile
hito.example.com {
    reverse_proxy 127.0.0.1:18080
}
```

Validate the actual host configuration before reloading Caddy. Do not replace its complete configuration or bind another process to ports 80 or 443. Export the exact public origin as `HITO_PUBLIC_ORIGIN` before running Compose; it has no default. `HITO_METADATA_CONTACT` is optional ([operations](../../docs/operations.md)). No additional origins or forwarded-client-IP trust are enabled. Login and setup rate limits currently share the reverse proxy's peer address. Keep request access logging disabled unless it has an independently verified redaction policy.

## Connect folders and plugins

In administrator Settings, Library, select **Add music folder**, browse the shared Music location and choose a folder. Repeat for separate subfolders. A browser selects only directories granted to this server; it cannot mount the user's computer through a native picker on the VPS.

For another drive, add a read-only bind mount at a distinct container path, add a manifest entry with a new stable `id`, `name` and matching `root`, then recreate the server. Keep existing IDs and paths unchanged. Do not mount the filesystem root, Docker socket or configuration directory as a music grant. Select the newly shared location in Settings. No `HITO_MUSIC_FOLDER` variable is needed.

Offer these internal addresses in Settings, Plugins. Each listener then connects personally, reviews capabilities and approves the connection. Leave Bearer token empty for these three services.

| Service address               | Current behavior                                                         |
| ----------------------------- | ------------------------------------------------------------------------ |
| `http://plugin-folder:8090/`  | Search and play generated demo WAV files                                 |
| `http://plugin-spotify:8092/` | Import or follow public playlists and retrieve artwork; no Spotify audio |
| `http://plugin-youtube:8093/` | Music search and metadata only; audio extraction is blocked on the VPS   |

Spotify artwork is part of the Spotify service and needs its artwork capability approved. Playback and automatic matching use the existing Hito workflows; providers do not render UI. Provider availability, extraction challenges and browser codec support can still fail. Linux amd64 and live provider acceptance must be checked on the actual images before claiming them verified. Real-Debrid is not included here; its current curated WAV service requires separate private configuration and is not general torrent support.

## Observe and recover

```sh
docker compose --env-file .env -f compose.yaml ps
docker compose --env-file .env -f compose.yaml logs --tail=100 server
curl --fail --silent --show-error http://127.0.0.1:18080/health
```

The SQLite health check and plugin manifest health checks come from the runtime images. Hito and plugin logs are structured and rotated, with three 10 MiB files per service. Keep normal logging at info. Never collect raw upstream responses, signed URLs, media, credentials or private metadata while diagnosing failures. Check host free space regularly. Explicit downloads must not be silently deleted to recover space.

For a failed plugin, inspect its safe outcome logs and restart only that service after resolving the cause. Other services and local playback can remain available. For server startup failure, preserve data and the last known image IDs. Diagnose permissions, disk space and migration results before retrying. Do not disable SQLite foreign keys, edit migration checksums or replace a database to make startup pass.

## Stop and back up

Back up both complete volumes while their owners are stopped. Do not copy a live `hito.db` alone. The server image contains tar, so no extra image needs pulling. These commands run from the private configuration directory with populated image variables loaded into the operator's shell:

```sh
set -eu
umask 077
backup_stamp=$(date -u +%Y%m%dT%H%M%SZ)
docker compose --env-file .env -f compose.yaml stop server plugin-folder
docker run --rm --pull never --network none --read-only --user 0 \
  --mount type=volume,source=hito-vps-data,target=/backup-source,readonly \
  --entrypoint tar "$HITO_SERVER_IMAGE" -C /backup-source -czf - . \
  > "/srv/hito/backups/${backup_stamp}-hito-data.tar.gz"
docker run --rm --pull never --network none --read-only --user 0 \
  --mount type=volume,source=hito-vps-folder-state,target=/backup-source,readonly \
  --entrypoint tar "$HITO_FOLDER_IMAGE" -C /backup-source -czf - . \
  > "/srv/hito/backups/${backup_stamp}-folder-state.tar.gz"
docker compose --env-file .env -f compose.yaml start server plugin-folder
```

Check each command's exit status before continuing. Preserve owners and modes, image IDs, revision, private `.env`, storage grants and original media separately. Encrypt or otherwise protect backups because they contain account data, sessions and credential keys. Transfer a verified copy off host. Keep enough free space for the next backup and incoming image archive; the initial VPS free-space observation was only about 9 GiB.

Restore into new empty volumes, retain the originals, and test the restored instance on an isolated loopback port without the public Caddy route. Use the matching image revision and keys. A successful archive alone does not establish restoration acceptance.

## Approve an upgrade

Updates are manual. Preserve the old immutable images and private configuration, take a verified stopped backup, load reviewed new amd64 images, then change the four pinned image IDs and revision together. Recreate with `up -d --no-build --pull never --wait`, verify health, login, library, plugins and playback, then retain evidence before retiring old images.

The VPS YouTube service runs with `--metadata-only` because YouTube blocks audio extraction from the datacenter IP address. It offers search and metadata only, and Hito no longer asks it for audio. When you upgrade an existing install to this command, each listener must open Settings, Plugins, choose Check connection, then Accept changes, to approve the removed `sources` capability. See [YouTube plugin](../../docs/plugin-youtube.md#metadata-only-mode).

An older binary may reject a database migrated by a newer image. Rollback requires the matching stopped pre-upgrade backup, its keys and its image revision in separate restored volumes. Changing an image tag alone is not a safe database rollback. Never run `down -v`, delete these volumes, prune unrelated images, or change another project's containers to free space.
