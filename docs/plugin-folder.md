# Music-folder plugin demo

This separate HTTP service works with Hito's current Settings and Discover workflows. The image contains three original generated sine waves, each 20 seconds long, with truthful titles, artist and album tags. It needs no provider account, paid service, token, shell environment variable, or change to Hito's database to start.

This is a development demo and an initial read-only folder adapter. The complete official folder-plugin release remains unfinished. In particular, tag-independent recording identity and durable release reconciliation are open.

## Start and connect

From the repository root containing the existing Hito Compose deployment, run:

```sh
docker compose -f compose.yaml -f compose.plugins.yaml up -d --build plugin-folder
```

This starts only the plugin service on the same Compose network as Hito. It does not install a plugin into an account. The host inspection endpoint is `http://localhost:8091/v1/manifest`. Hito must use its internal Docker address:

```text
http://plugin-folder:8090/
```

1. Sign in to Hito as an administrator. Open Settings, then Plugins.
2. Expand Services for this server. Enter `Folder demo` and the internal address above, then select Offer service.
3. Select that offered service, leave Bearer token empty, and select Connect service. Review the displayed `Hito music folder` identity and its `search` and `sources` capabilities. Check connection revalidates the manifest. Each listener personally installs their own connection.
4. Open Discover, select the service, and search for `Hito demo`. The tracks are synthetic test signals, not commercial music.
5. Select Add to library to save a recording without starting playback. Select Play to adopt and play another track. Use the Seek control to change position.
6. In Settings, Plugins, select Remove, then Remove plugin. Saved recordings remain in the library and become unavailable through this connection.

Hito retains its administrator-only address offers, personal installation, and authenticated audio relay. The browser receives Hito source UUIDs. It receives no plugin URL, token, filename or directory locator.

The demo has no bearer authentication. The port binds only to localhost, and the service is reachable by containers on its Compose network. An offered collection is available to accounts allowed to install that offer. Keep this configuration on a trusted local machine and network. A remotely shared collection needs transport and access controls before exposure.

## Use a read-only collection

Create `compose.plugins.local.yaml` with the folder you own:

```yaml
services:
  plugin-folder:
    volumes:
      - type: bind
        source: /absolute/path/to/your/music
        target: /collection
        read_only: true
    command: ["--music", "/collection", "--listen", "0.0.0.0:8090"]
```

Run:

```sh
docker compose -f compose.yaml -f compose.plugins.yaml -f compose.plugins.local.yaml up -d --build plugin-folder
```

The service scans at startup. Restart it after collection changes. The container runs as UID 10001 and needs read and directory traversal access to the selected collection. It never edits that collection. Remove the local override from the command to return to the generated demo.

Tags supply title, artist, album, album artist, disc and track numbers. Untagged audio gets a neutral opaque label and `Unknown artist`; filenames never become metadata. Only byte-identical files collapse. Similarly named performances remain separate. Tagged tracks in one folder with the same album and album artist share a release; different folders remain separate unless their complete audio/tag evidence is identical. Multi-disc grouping across folders remains open.

Recognized original representations are WAV, FLAC, MPEG audio, MP4 audio, AAC, Ogg Vorbis/Opus and AIFF. Recognition does not prove every codec will decode in every browser. The owned WAV demo is the tested browser representation. There is no transcoding in this service.

## Ownership and limits

```mermaid
flowchart LR
    Settings[Personal Settings connection] --> Hito[Hito plugin authority]
    Hito --> HTTP[Folder service fixed HTTP endpoints]
    HTTP --> Catalog[Startup tag and file catalog]
    Catalog --> Handles[Read-only directory handles]
    Handles --> Collection[Owned collection or generated demo]
    Hito --> Relay[Authenticated audio relay]
    Relay --> HTTP
```

The plugin uses the Apache-2.0 wire SDK and owns its scanner and file handles. It has no dependency on Hito core, SQLite or its storage internals. The manifest declares only search and sources. It implements manifest, paged search, authoritative recording detail, complete source lists, and GET/HEAD audio with single byte ranges. HEAD ignores Range. If-Range returns a full representation. Multipart, malformed and unsatisfiable byte ranges return 416. Responses use private no-store caching.

Scanning permits at most 50,000 directory entries, 1,024 folders, 32 levels, 10,000 distinct recordings, 1 GiB per file, and 16 GiB of inspected accepted files, with a five-minute deadline checked during reads and seeks. Metadata reads have an 8 MiB byte budget; cover art is not read. Unsupported or unreadable media is skipped and counted. Deadline, metadata-budget and collection-limit failures stop startup rather than publishing a partial catalog. The scanner checks its deadline again before publication. The startup log reports counts and elapsed time, without paths or metadata.

The service skips symlinks and non-regular files. An owned parent handle opens the selected root with one no-follow operation, rejecting a replacement symlink at that boundary. This does not pin mutable ancestor pathnames before the parent is acquired. No-follow child directory handles preserve containment, and audio requests select an existing opaque source ID rather than a path. Reopened files must match their scanned inode, change time, modification time and size on Linux. Replaced or removed files become unavailable. A read-only Docker mount prevents plugin writes, though the host owner can still modify the collection. It is not an immutable audio snapshot during a host edit.

JSON bodies are limited to 4 KiB, search results to the protocol's 256 KiB budget, and request handling to 16 concurrent requests with ten-second handler/body deadlines. Four file-open workers and eight audio streams have separate admission limits. Streaming reads at most 64 KiB at a time and respects backpressure. Each blocking read worker owns its file and admission permit. If a consumer closes during a pending read, capacity remains occupied until that read finishes and the worker closes the file. Logs use safe operation categories, correlation IDs, outcomes and timings. Input rejection omits parser details even at TRACE. Production logging also hard-filters all dependency diagnostics, including parser messages bridged from `log`, independently of `RUST_LOG`; safe plugin-owned counts and outcomes remain available.

## Identity work still open

Current recording and source IDs derive from the exact file bytes. Moves and renamed roots keep these IDs. Retagging changes bytes, so the current service creates a different provider recording identity after restart. Hito will retain the earlier adopted recording, but this demo does not reconcile it with the retagged file. Do not treat this as completed recording identity or acoustic matching.

Release IDs hash album tags and the sorted recording identities in one folder group. They contain no path or directory name. This keeps the immutable demo album coherent and survives moving its whole unchanged folder. Adding, removing or retagging album members can change the release ID. Persistent release continuity remains open.

The next identity slice should keep exact source revisions separate from persistent opaque recording IDs and versioned decoded-audio evidence. Only an unambiguous verified audio match should preserve a recording after retagging; unsupported and ambiguous media must remain separate. That requires persistent plugin-owned state, bounded decoding and recovery tests, without importing core SQL internals. The current service intentionally has no state database or background watcher.

## Verification

The focused check uses the existing Hito tools image and volumes, a separate `/build/plugin-folder-target`, two compile jobs and disabled incremental compilation:

```sh
./scripts/check-plugin-folder.sh
```

The recorded focused run passed sixteen folder-service tests and one SDK contract test, with formatting and strict Clippy clean. A real TCP test also verifies deferred stream logging retains request correlation and a scoped subscriber, then checks graceful server shutdown. Boundary tests cover the real SDK contract, complete paging, large-tag response limits, conservative matching, coherent releases, root moves, symlinks, special files, traversal, changed sources, malformed requests, TRACE privacy, single ranges, HEAD and abandoned-stream cleanup.

For the isolated browser flow, first have Hito's development tools and `hito-e2e-server:latest` image available. This browser configuration mounts the existing `hito_node-modules` dependency volume. It uses a fresh tmpfs database and its own administrator account, never the normal server volume:

```sh
docker compose -p hito-plugin-folder -f compose.plugins.yaml build plugin-folder
docker compose -p hito-plugin-folder -f compose.plugins.yaml -f scripts/plugins/compose.e2e.yaml --profile e2e run --rm plugin-folder-browser
docker compose -p hito-plugin-folder -f compose.plugins.yaml -f scripts/plugins/compose.e2e.yaml --profile e2e down
```

The browser test uses real UI offer/install, search, adoption without autoplay, playback, seek, relayed byte ranges, one shared durable album, and personal removal with durable recording IDs retained. Screenshots go to `test-results/plugins-folder/`. The reused Hito integration image for the recorded run was `sha256:028340162faea37e3309a6bdc6db029977aaf727ef367b2974de185bd8300dbe`, built on 2026-10-16. This proves the existing HTTP plugin boundary, not newer native-speed changes or a public release.

Jamendo, YouTube/yt-dlp, debrid, community, mobile, cross-platform packages, large-library performance and the rest of the release plan remain separate work.
