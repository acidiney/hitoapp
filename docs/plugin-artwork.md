# Provider artwork in development

Hito can retain provider covers in its private SQLite library. The server worker and shared web UI can retrieve missing recording covers, including recordings already present in independent playlist copies. The initial recording-cover deployment on migration 0027 passed stopped-backup, clone-restoration, data-preservation and runtime checks. Existing connections retain their approved capabilities; each listener must review an added artwork capability before it can be used.

The plugin SDK declares an `artwork` capability and a typed `POST /v1/artwork` request containing an entity kind and provider identifier. Plugins return PNG or JPEG bytes. They do not return a browser URL or supply UI components. The reference Spotify implementation supports recording, release, and playlist requests without Spotify account authorization.

## Album and imported-playlist covers in source

The integration source extends the existing worker to recordings, releases and imported editable playlists. Each target has its own account-owned reference and receipt. Album requests pin an exact provider release witness. Playlist requests pin the actual import's provider identity, snapshot and import incarnation; they do not invent a recording to hold the cover. All three targets share transport, decoding, quota, lease, publication and retention rules.

Album pages and imported-playlist pages read private cover bytes through authenticated Hito routes. Playlist lists use the playlist's own cover rather than borrowing its first recording. Manually created playlists have no implicit provider, and Follow artwork remains unfinished. No plugin supplies UI or browser image URLs.

Image loading and provider retrieval are separate states. An explicit Retry immediately attempts the retained private image, including after provider disconnection. A ready receipt does not start another provider job merely because an image load failed. Failures stay visible for that image revision even if another renderer loads the same cover successfully. A new account-provider generation retires old image events. Retrying an evicted terminal entry reconstructs it through the same bounded cache without automatic retries or extra capacity.

The independent final frontend repeat passes 472 cases across 148 files, both TypeScript configurations and the production build. Directed DOM controls reproduce and correct ready-image failure, disconnected-provider recovery, same-component account changes, two renderers with different image outcomes, and Retry after 129 settled targets. The last case retains the 128-entry cache and two-request limit. Two correctness review rounds found defects that received failing regressions and passing fixes; no third clean review is claimed.

Core fixtures also exercise the shared worker with 100,000 retained targets and one shared blob under inherited, absent and fresh SQLite statistics. Read, enqueue and worker work stay below their existing 10,000, 250,000 and 500,000 VM-step budgets. A deterministic pinned-reader case proves that fixture snapshots include committed WAL history; it preserves rows, row IDs, retained bytes, foreign keys and the intended statistics. These are query-work and fixture-consistency checks, not playback latency or public backup acceptance.

Complete combined backend, packaged-browser and normal-deployment acceptance remain in progress. The normal server still runs migration 0046 and its recording-cover behavior described below.

```mermaid
sequenceDiagram
    participant Caller
    participant Catalog
    participant SQLite
    participant Plugin
    Caller->>Catalog: Enqueue artwork for an owned recording
    Catalog->>SQLite: Check session, metadata owner, quotas; save receipt
    Catalog-->>Caller: Stable queued or retained receipt
    Note over Caller,Plugin: Enqueue and status reads make no provider request
    Catalog->>SQLite: Worker claims a session-bound lease
    Catalog->>Plugin: Bounded binary artwork request
    Plugin-->>Catalog: Original image bytes
    Catalog->>Catalog: Decode and check framing and dimensions
    Catalog->>SQLite: Recheck authority; publish cover and ready receipt atomically
    Caller->>Catalog: Read retained artwork
    Catalog-->>Caller: Authorized private bytes
```

## Ownership and retention

Retained artwork is account-owned. A recording fetch pins the selected provider metadata observation, binding incarnation, installation revision, service revision, and initiating session. A worker checks that authority during retrieval and again in the publication transaction. Logout or withdrawn authority prevents publication. Network and decoding work happen outside the write transaction.

Local file artwork remains preferred. A retained provider cover remains readable offline and after the plugin is removed. Retrying a completed request for the same selected metadata owner returns its original ready receipt. Each retained cover protects that exact receipt from history cleanup. Changing metadata ownership can require a fresh fetch while the previous cover remains readable.

Retrieval does not reimport a playlist, adopt a recording, download audio, or change playlist entries, likes, progress, or personal metadata. Independent copies and duplicate occurrences keep their identities.

## Bounds and lifecycle

- Encoded images are at most 2 MiB, with dimensions from 1 to 1,024 pixels and an output buffer of at most 8 MiB.
- PNG decoding checks critical and ancillary CRCs through the end of the input and rejects animation. JPEG decoding uses the locked zune strict mode, at most 64 progressive scans, and exact consumed-input and EOI checks.
- Decoder limits bound specific buffers and work. They do not establish a total process allocation or decoding time limit. The JPEG dependency can still pad some prematurely terminated entropy data; these checks do not prove rejection of every malformed JPEG.
- There are at most 16 pending requests per account, 128 globally, one active lease per account, and four globally. Background provider admission is separate from interactive requests. Local decode permits are acquired after the image download.
- Image assets are limited to 32 MiB and 1,024 distinct assets per account, or 256 MiB and 8,192 globally. Pending requests reserve their maximum encoded size.
- Fetches allow three attempts with a 60-second lease. Provider transport has a ten-second request deadline. Read-only authority checks have a one-second deadline. Publication transactions are awaited rather than abandoned on timeout.
- Joined shutdown requeues interrupted work without consuming an attempt. Ready covers remain retained. Disposable terminal history expires after seven days and has account and global caps. Current retained receipts grow with the recording library.

Status polling uses a scoped read transaction and does not run global cleanup or acquire the SQLite writer. Idle worker polling checks indexed due jobs and expired leases before taking a writer transaction. Cleanup still runs from enqueue and active worker paths. Its work at the full release load remains to be measured.

## Browser retrieval

The shared account-scoped UI checks missing recording covers through `GET /api/v1/recordings/{id}/artwork/availability`. This read checks the current approved metadata provider without creating a job, taking the SQLite writer or contacting the plugin. An eligible missing image starts the explicit enqueue/status workflow. Local file and release images remain preferred.

Duplicate occurrences and different components share one retrieval for each recording. The browser admits two HTTP requests at a time, including response decoding, and at most 16 workflows. Its cache and waiting queue share a 128-entry bound. Status polling runs every second for at most two minutes. Terminal failures require the track-details Retry artwork action. Navigation and account changes cancel observers and suppress late responses; physical request slots stay occupied until their promises settle.

After an explicit artwork-capability approval is acknowledged, the UI clears that account's completed unavailable checks. Visible observers retry through the existing bounded workflow; other covers wait until viewed. Reviewing changes alone does not enable artwork. Ready covers, active jobs and terminal failures retain their state. This fix is deployed on the normal local server. A browser already running the earlier UI must reload once to receive the new code.

The player dock, expanded Now Playing, queue rows and the browser Media Session use the same release-then-recording order as track rows. A missing Media Session cover starts this same retrieval; see [browser Now Playing](browser-media-session.md).

More than 128 simultaneous missing covers can leave placeholders until remount or an explicit retry. Large-library visibility and paging behavior still need release-load verification.

## Verified evidence and remaining work

The album/imported-playlist source also passes the isolated packaged Chromium and WebKit gate, with an independent repeat and fresh review. Each run preserves duplicate independent copies, private edits and original files, verifies four durable artwork jobs, and checks ten exact authenticated download-status absences. Chromium separately qualifies later-renderer success while a failed hero retains Retry; WebKit qualifies ordinary coalesced failure and explicit Retry recovery. The immutable product image, overlay/source checks, reproduction commands and exclusions are recorded in [the packaged gate](../scripts/artwork/packaged-browser.md). Followed-playlist artwork, live-provider acceptance and normal publication remain open.

The approval-cache change passed 244 frontend cases across 78 files, both TypeScript checks and the production build. A held acknowledgement/account-switch regression verifies that an old account cannot retry another account's missing cover. Two independent source/test reviews found no concrete defect. The packaged Chromium workflow passed in 6.1 seconds with a document token proving uninterrupted SPA navigation through approval and return to the playlist. It decoded the covers, preserved saved state, retained them after provider removal and rejected another account's reads. Download mutations remained zero. A scoped scan of 4,615 isolated server-log records found none of the checked fixture credentials or metadata. This does not establish physical Safari, native or release-load acceptance.

The browser check also exposed two older test assumptions. An unchanged baseline reproduced both: Play is enabled for known unavailable recordings so an explicit click can resolve audio, and an absent playlist-download status returns 404. The test now expects enabled controls and permits only exact owned download-status GET URLs, preserving separate artwork errors and console correlation. It does not explicitly reject an otherwise successful source-resolution request without audio; that remains a coverage limitation.

Public core cases cover private retention across independent copies and restart, unchanged collection state, logout and provider removal during held image bodies, joined shutdown, corrupt PNGs, incomplete JPEGs, retained-receipt cleanup, unrelated writer contention, idle polling under a held writer, and interactive searches and local covers during four background downloads. A real TCP host test verifies automatic worker retrieval, retained bytes, idempotent retry, account isolation and joined shutdown. The read-only eligibility endpoint has a separate account/session and no-side-effect HTTP regression.

The final frontend passed 149 cases across 32 files, both TypeScript checks and its production build. Its regressions cover shared retrieval, bounded admission, explicit retry, account changes, late responses and navigation away and back. Independent security and performance reviews passed. Correctness review found a navigation recovery defect, which passed its failing regression and second review.

One actual release-image Chromium workflow passed in 6.6 seconds. It offered an address, connected with a private token, created two independent copies with four duplicate entry occurrences, approved the added artwork capability and retrieved a held PNG. The images loaded through Hito's authorized routes in both copies without changing playlist identities, personal metadata, likes, progress or synchronization events. The same covers remained readable after removing the connection and making the provider unavailable. Another account could not read the images or receipts. Desktop and 390-pixel layouts and focus checks passed, and the captures were inspected. The deterministic cover is a plain fixture, not a visual design acceptance test.

A scoped scan of 831 captured server-log lines found no owned fixture metadata, references, tokens, binary provider paths or bearer values. The browser workflow separately checked its console, storage and request URLs. These checks do not establish complete production-log privacy.

Two independent core foundation review rounds found issues that received failing regressions and passing resolver checks. There is no third independent clean verdict. Host integration passed its second review. These checks do not establish platform or public-release acceptance.

The normal deployed host currently retrieves recording covers; the collection source above is not deployed. A packaged live producer check decoded one Spotify recording cover and one playlist mosaic at 640 × 640. Follow covers, broader image compatibility, crash and disk-exhaustion behavior, the 100,000-track release gate and public-release deployment acceptance remain open. See [the complete release scope](release-plan.md).

## YouTube thumbnails in source

The YouTube plugin supplies a recording's `hqdefault.jpg` thumbnail through this same capability, in full and metadata-only modes. A saved YouTube recording becomes fetchable only after the listener approves the added capability (Check connection, then Accept changes). A core test confirms a YouTube-shaped manifest makes availability true and that one without `artwork` stays false. Live thumbnail retrieval from the VPS is unverified. See [YouTube plugin](plugin-youtube.md#thumbnails-as-artwork).
