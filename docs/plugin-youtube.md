# YouTube plugin development

The separate YouTube HTTP service implements search, recording details, musical metadata, AAC/M4A sources and audio relay. Its manifest declares `search`, `metadata` and `sources`. Plugins return structured metadata and audio; Hito owns the interface. The verified image runs on the normal local Docker network. Address-only installation, discovery, saving, playback, reviewed attachment and one real automatic Spotify-to-YouTube target pass in WebKit against disposable libraries. Each listener connects personally; deployment did not authenticate or change a normal account.

## Isolated Docker service

Build and start the service in its own project:

```sh
HITO_YOUTUBE_PORT=18093 docker compose -p hito-youtube-runtime -f compose.youtube.yaml --profile plugins build plugin-youtube
HITO_YOUTUBE_PORT=18093 docker compose -p hito-youtube-runtime -f compose.youtube.yaml --profile plugins up -d --no-deps --no-build --wait plugin-youtube
```

The manifest is available at `http://localhost:18093/v1/manifest`. This isolated project is separate from the normal Hito server network and has no Hito database or music mounts. Do not use the normal persistent server for first-run acceptance tests.

The image pins yt-dlp 2026.08.19 and Deno 2.9.7 for Linux GNU amd64 and arm64. The installer verifies SHA-256 before unpacking each release asset. It retains yt-dlp's complete runtime bundle and bundled notices, plus Deno's root MIT license and available notices from its pinned source archive. The complete dependency attribution and redistribution audit remains a release requirement.

The service runs as UID 10001 with a read-only root filesystem, no extra capabilities, a 64 MiB temporary filesystem, a 128-process limit and a 512 MiB memory limit. The temporary filesystem disallows executable files. Deno and yt-dlp are installed in the read-only image; requests do not install runtimes or remote components.

## Testing on the normal local installation

The normal service runs image `sha256:497267e06352260f159ae0410869cd9dcbe5302b9ba5baedae3f1444305fa797`, built from `2b4ad7e`, at `http://plugin-youtube:8093/`, with a loopback host manifest at `http://localhost:8093/v1/manifest`. It searches the music catalog. Its health, manifest and reachability from the normal server passed. The runtime retains its exact environment, command, network, ports, read-only root, UID 10001, dropped capabilities, temporary-filesystem and resource limits, with no Hito data or music volume.

In Settings, Plugins, offer that internal address and connect without a bearer token. Existing connections use Check connection, then Accept changes to approve the new metadata capability. Discover can search and save YouTube recordings. Ordinary Play on a saved Spotify recording can now resolve its audio automatically. For a correction, open its details, use Find a source to review and attach an alternative, then choose that playback source. Both paths retain the recording and playlist identities and Spotify metadata owner. Resolution happens when playback needs it; importing a playlist does not start persistent downloads. See [automatic matching policy and live evidence](automatic-sources.md).

The music-search update keeps the existing manifest and permissions. An already approved metadata connection needs no reinstall or additional approval for this update.

## Process ownership

```mermaid
flowchart LR
  Request[Search request] --> Admission[Two admitted extraction workers]
  Admission --> Child[Owned yt-dlp process group]
  Child --> Bounds[Bounded output and eight-second deadline]
  Bounds --> Metadata[Validated SDK metadata]
  Metadata --> Reply[Search response]
  Cancel[Cancellation or shutdown] --> Cleanup[Terminate descendants and reap leader]
  Cleanup --> Release[Release worker capacity]
```

The extractor passes literal arguments without a shell and clears the inherited environment. It disables configuration, cookies, browser cookies, plugin directories, remote components, downloads and disk caching. Search uses the YouTube Music songs catalog, with an encoded query and `--playlist-end` set to the requested limit, at most 50. This is music-catalog discovery; ordinary YouTube video search is not exposed. Existing explicit video identities still use the same detail and audio endpoints. Output is limited to 2 MiB on stdout and 64 KiB on stderr, then normalized to a maximum 256 KiB SDK response. Search terms and upstream output stay out of operational logs.

Flat music results contain only identity and title. When artist evidence is absent, search returns `Unknown artist` with an optional duration. Detail extraction remains strict and independent. The plugin never fills musical credits or track titles from the listener's query.

The worker retains its capacity until process-group cleanup finishes, including when the HTTP caller disconnects. Shutdown stops admission, cancels workers and joins cleanup. A cleanup failure stops further extraction. These process-ownership checks currently target Linux GNU, which is the service container platform.

## Audio ownership and bounds

Recording details and sources use a canonical eleven-character video identifier. A source ID identifies the selected format, length and upstream modification witness. It contains no signed URL. The 128-entry LRU cache can reextract an evicted or expired item; a changed representation cannot reuse an old source ID. These witnesses do not establish cryptographic content identity.

Only original, progressive HTTPS AAC LC in an M4A container is supported, up to 128 MiB. The extractor does not pass its returned cookies or authorization headers into audio requests. Audio endpoints accept HEAD, full GET and one byte range. Hito verifies upstream status, length, range, media type and available validators before sending audio. Conversion and other codecs remain open.

```mermaid
flowchart LR
  Sources[Typed source request] --> Extractor[Owned extractor and bounded cache]
  Extractor --> Descriptor[Private validated media descriptor]
  Audio[Audio request] --> Admission[Four relay permits]
  Admission --> Descriptor
  Descriptor --> Driver[Owned HTTP/1 driver and verified TLS socket]
  Driver --> Output[Original bytes in frames up to 64 KiB]
  Cancel[Body drop or service shutdown] --> Close[Drop driver and socket]
  Close --> Release[Release relay permit and tracked work]
```

The HTTP connection driver stays inside the response owner; it is not spawned into a detached client task. Header requests are bounded by the existing eight-second open budget. DNS, TCP and TLS connection establishment has a two-second timeout; HTTP parsing allows 32 fields, 16 KiB of headers and a 256 KiB read buffer. These async deadlines do not establish a bound on underlying operating-system resolver work.

A ten-second idle deadline applies while actively waiting for upstream audio. Downstream pauses do not consume that deadline, and buffered bytes can resume after a pause. Incoming frames larger than 64 KiB are split without copying their payload. Admission remains held with the owned stream and pending bytes until cleanup. Shutdown closes retained bodies and waits for admitted opens and process cleanup.

## Verification and remaining work

Sixteen fixture cases passed through Docker, together with formatting and strict all-target Clippy. They cover executable SIGTERM and descendant cleanup, the exact child environment, metadata/source validation, cache eviction and changed representation identity, full AAC delivery, ranges, native decoding, four-body admission, consumer pauses, active input timeout, large real HTTPS frames, correlated completion logs and INFO/TRACE privacy. A current-thread Linux regression observes the concrete client socket inode and verifies it is closed before shutdown returns, even while the caller retains an unpolled response body.

Two review rounds found defects that received failing public-boundary regressions and passing resolver checks. The final ownership fix uses the direct HTTP driver described above. No third independent clean verdict is claimed.

The packaged arm64 image `sha256:fe3f2abaff3df419a332bc3d34ad3d037a7b79afee154ea6e2ed77291b381178` is healthy in the isolated project. An explicitly opted-in live test passed SDK details/sources, HEAD, a nonzero byte range, full transfer and native Symphonia decoding. It received 527,856 encoded bytes and decoded 2,871,296 finite samples at 44.1 kHz stereo with nonzero energy. HEAD, range and full transfer together took 16,291 ms; decoding took 316 ms. The recording was about 33 seconds long. These are functional observations, not playback-start or release latency measurements.

The ignored test requires `HITO_YOUTUBE_LIVE=1`, an explicit isolated `HITO_YOUTUBE_LIVE_BASE_URL` and a canonical public `HITO_YOUTUBE_LIVE_VIDEO_ID`. It accepts only loopback or named isolated-service endpoints, writes encoded bytes to a mode-0600 temporary file, bounds transfer and decode work, and prints numeric evidence without titles, identifiers, URLs or paths. Invoke it through the Docker Rust tools service with `cargo test --locked -p hito-plugin-youtube --test live_audio -- --ignored --exact release_image_relays_public_aac_with_ranges_and_native_decoding --nocapture`.

An explicitly opted-in release-image WebKit workflow passed in 21.7 seconds. It offered the service address, connected without a bearer, searched, saved the recording, played AAC through the authorized Hito source URL, sought to 7.9 seconds and continued through navigation. Presented time reached 8.33 seconds with media error zero. No browser request reached the plugin or media CDN. This sample sought within buffered audio; the separate live HTTP check above exercises a nonzero upstream byte range.

The test distinguishes WebKit's headerless synthetic network observation from a real HTTP response. It requires that observation to finish normally and match an actual same-URL 200/206 response with valid audio framing. Zero is never counted as HTTP success. WebKit can also cancel a progressive read and resume through increasing open byte ranges. A separate 17-row fixture table verifies the narrow classification of its exact cancellation code, consistent source/total/framing, chronological range replacements and normally completed final tail. Raw cancellations remain counted separately from unexpected failures. Media errors, malformed responses, unexplained failures, external URLs and stalled presentation still fail the workflow.

A real public Spotify playlist workflow passed in 12.8 seconds. It imported 411 ordered entries, explicitly reviewed and attached one conservatively selected YouTube candidate, played AAC, sought to 57.5 seconds and continued through navigation. The recording, playlist order, Spotify metadata owner and personal edits remained unchanged. The other 410 entries stayed visibly unavailable. Ten browser range replacements were distinguished from zero unexpected request failures; media error remained zero. This demonstrates manual association, not automatic matching or general provider compatibility. The final setup guard passed a separate owned-server regression: an initialized server is refused before credential input or POSTs, while account and collection snapshots remain unchanged. The guarded live workflow then passed again in 13.0 seconds. Independent correctness and performance reviews passed; a security finding received the failing regression, fix and passing second review. The browser spec separates provider setup, collection invariants and playback checks into cohesive helpers.

The packaged Chromium test browser reports no AAC support. The packaged Firefox advertises AAC but raises a media decode error in the real workflow. These are unresolved compatibility limits of the current test images; they do not establish behavior in every installed browser. Conversion and broader browser acceptance remain open. Source and browser-test TypeScript and formatting passed. Scoped scans of 1,316 Hito and 562 plugin log lines found no configured video identifier, signed media host/expiry or bearer values.

The amd64 runtime, broader upstream and challenge compatibility, expiry and range-recovery countercases, automatic playlist-wide association, other codecs, physical output and platform/public-release acceptance remain open. Complete runtime dependency attribution and redistribution acceptance are also still required.
