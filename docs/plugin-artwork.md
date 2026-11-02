# Provider artwork in development

Hito can retain provider covers in its private SQLite library. The server worker and shared web UI can retrieve missing recording covers, including recordings already present in independent playlist copies. The initial recording-cover deployment on migration 0027 passed stopped-backup, clone-restoration, data-preservation and runtime checks. Existing connections retain their approved capabilities; each listener must review an added artwork capability before it can be used.

The plugin SDK declares an `artwork` capability and a typed `POST /v1/artwork` request containing an entity kind and provider identifier. Plugins return PNG or JPEG bytes. They do not return a browser URL or supply UI components. The reference Spotify implementation supports recording, release, playlist and artist requests without Spotify account authorization.

## Album and imported-playlist covers in source

The integration source extends the existing worker to recordings, releases and imported editable playlists. Each target has its own account-owned reference and receipt. Album requests pin an exact provider release witness. Playlist requests pin the actual import's provider identity, snapshot and import incarnation; they do not invent a recording to hold the cover. All three targets share transport, decoding, quota, lease, publication and retention rules.

Album pages and imported-playlist pages read private cover bytes through authenticated Hito routes. Playlist lists use the playlist's own cover rather than borrowing its first recording. Manually created playlists have no implicit provider, and Follow artwork remains unfinished. No plugin supplies UI or browser image URLs.

Image loading and provider retrieval are separate states. An explicit Retry immediately attempts the retained private image, including after provider disconnection. A ready receipt does not start another provider job merely because an image load failed. Failures stay visible for that image revision even if another renderer loads the same cover successfully. A new account-provider generation retires old image events. Retrying an evicted terminal entry reconstructs it through the same bounded cache without automatic retries or extra capacity.

The independent final frontend repeat passes 472 cases across 148 files, both TypeScript configurations and the production build. Directed DOM controls reproduce and correct ready-image failure, disconnected-provider recovery, same-component account changes, two renderers with different image outcomes, and Retry after 129 settled targets. The last case retains the 128-entry cache and two-request limit. Two correctness review rounds found defects that received failing regressions and passing fixes; no third clean review is claimed.

Core fixtures also exercise the shared worker with 100,000 retained targets and one shared blob under inherited, absent and fresh SQLite statistics. Read, enqueue and worker work stay below their existing 10,000, 250,000 and 500,000 VM-step budgets. A deterministic pinned-reader case proves that fixture snapshots include committed WAL history; it preserves rows, row IDs, retained bytes, foreign keys and the intended statistics. These are query-work and fixture-consistency checks, not playback latency or public backup acceptance.

Complete combined backend, packaged-browser and normal-deployment acceptance remain in progress. The local development server ran migration 0046 when this was written and now runs through 0069 ([status](status.md#2026-10-29-to-2026-10-31-summary)).

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

The local development host retrieves recording covers. It was deployed on migration 0046; whether the collection source above is live has no separate record. A packaged live producer check decoded one Spotify recording cover and one playlist mosaic at 640 × 640. Follow covers, broader image compatibility, crash and disk-exhaustion behavior, the 100,000-track release gate and public-release deployment acceptance remain open. See [the complete release scope](release-plan.md).

## YouTube thumbnails in source

The YouTube plugin supplies a recording's `hqdefault.jpg` thumbnail through this same capability, in full and metadata-only modes. A saved YouTube recording becomes fetchable only after the listener approves the added capability (Check connection, then Accept changes). A core test confirms a YouTube-shaped manifest makes availability true and that one without `artwork` stays false. Live thumbnail retrieval from the VPS is unverified. See [YouTube plugin](plugin-youtube.md#thumbnails-as-artwork).

## Provider covers for imported albums

An album imported from a provider playlist has no MusicBrainz identifier, so the Cover Art Archive cannot supply its cover. Reading an album cover (`GET /api/v1/releases/{id}/artwork`) now queues the provider release request itself when nothing local, retained or archive-held exists. Core queues it only when the album's recordings carry a provider release identity from an approved connection whose manifest declares `artwork`; a pending or failed request for the album is not repeated, and the existing per-account (16) and global (128) pending limits apply, so a long album list backfills a few covers at a time. The server worker fetches them through the same lease, validation and retention rules as an explicit request, bound to the reading session. Existing imports need no reimport; they fill in as their albums are shown. A request made with no session (nothing is reading) is not possible, so there is no sessionless background sweep.

The Spotify plugin supplies the cover through its existing release artwork request: it reads the public album embed page, takes the largest listed image and fetches it only from the exact Spotify image hosts, with no redirects and the 2 MiB, 1,024-pixel limits above. It makes no call that needs a Spotify account, and the playlist response it already parses carries no image URLs in the owned fixtures, so none are taken from it. Only the plugin and the image host are contacted; logs carry job and account identifiers, not titles, provider ids or URLs.

The choice **Fetch album artwork automatically** also governs this: when it is off, reading a cover queues nothing and `GET /api/v1/releases/{id}/artwork/availability` answers `can_fetch: false`, so the browser does not start a retrieval. An explicit `POST /api/v1/releases/{id}/artwork/fetch` still works. Covered by core (owned fake artwork provider) and HTTP cases. The server code runs on the local development server, but the Spotify container uses an older image until it is rebuilt (see [Spotify plugin](plugin-spotify.md#local-setup)) and the listener must approve the artwork capability.

## Cover Art Archive

Albums and discography cards can show a front cover without any plugin. When a release or release group has a MusicBrainz identifier, Hito reads `front-500` from coverartarchive.org. Only the identifier is sent (no title, artist, path or account). This is a built-in client in `hito-metadata-musicbrainz`, not a plugin capability: it follows redirects only to https `archive.org` hosts (at most three), accepts only JPEG or PNG up to 2 MiB, and makes at most one request per second. The image then goes through the same validation as provider artwork (format, dimensions, decode) and is stored in the account's artwork store.

Where identifiers come from: the current MusicBrainz release or release-group tags of a library album (see [tag identifiers](artist-matching.md#release-and-recording-identifiers-from-tags)) and the release groups of a built-in MusicBrainz discography page. Requests are queued in `cover_art_lookups` when an album without artwork is read, after an automatic matching walk for albums with no local files, and when a discography page is read, at most 64 pending per account. A worker runs them one at a time.

Honest states per identifier: `queued`/`running` (pending), `ready`, `missing` (the archive has no cover, or sent an invalid image), `unavailable` (four failed attempts with growing delay). Missing and unavailable answers are asked again only after a day, and only when something asks again. `GET /api/v1/cover-art/{kind}/{mbid}` (kind `release` or `release-group`) never contacts the archive: it returns the image (200), 202 while pending, 404 when missing or never asked, or 502 when unavailable. `GET /api/v1/releases/{id}/artwork` falls back to this cover after embedded and provider artwork.

## Artist photos

MusicBrainz has no artist images, but its URL relations link an artist to Wikidata, and Wikidata may name a Commons file in property P18. `ArtistImageClient` (in `hito-metadata-musicbrainz`) follows exactly that chain on the server: the artist's Wikidata entity from the paced MusicBrainz client, then `https://www.wikidata.org/wiki/Special:EntityData/<Q>.json` (4 MiB limit, the preferred non-deprecated P18 claim), then `https://commons.wikimedia.org/wiki/Special:FilePath/<file>?width=320`. The file name is checked (at most 240 bytes, no control characters, `/`, `\`, `#` or `?`) and sent as one encoded path segment. Redirects (at most three) may reach only `wikidata.org`, `www.wikidata.org`, `commons.wikimedia.org` and `upload.wikimedia.org` over https without a port or credentials. The image must be a JPEG or PNG of at most 2 MiB whose content type and magic bytes agree, then passes the same decode and 1,024-pixel validation as every cover. Requests start at least half a second apart. Only the MusicBrainz and Wikidata identifiers leave the host; no name, title, path or account.

Photos share the cover lookup queue (`cover_art_lookups`, kind `artist`, migration 0080 rebuilds the table so its kind check admits it), the 64-pending limit, the retry states and the account's artwork store. `GET /api/v1/cover-art/artist/{mbid}` queues a lookup the first time it is asked (202), then answers 200 with the stored image, 404 when there is none or **Fetch album artwork automatically** is off, or 502. The browser retries a failed image twice, 2 s and 5 s later. A `missing` or `unavailable` photo is asked again only when the route is read more than a day after its last attempt, the same aging as covers. At most 32 artist lookups per account may be queued or running at once (covers share a 64 lookup limit); further identifiers answer 404 until some finish, so a stray request for unknown identifiers cannot flood the queue. A stored photo is served only while the choice is on. With the choice off nothing is queued and no request leaves the host. The Spotify artist page relation is not used for these MusicBrainz photos; credits known only through a plugin use the separate path in [photos from a connected plugin](#photos-from-a-connected-plugin).

Covered by owned loopback fakes of MusicBrainz, Wikidata and Commons (five core cases including a refused redirect, an HTML answer, a wrong signature, an unavailable service and an oversized body; an upgrade case for migration 0080; one HTTP case) and never against the live services.

### Photos from a connected plugin

Current behavior in source (not deployed; the running server has neither migration 0081 nor a rebuilt Spotify container). An artist that Hito knows only because a plugin credited it, such as a performer on an imported Spotify playlist, has no MusicBrainz identifier, so the chain above cannot give it a photo. The `artwork` capability therefore has a fourth request kind, `artist`: `{"entity":"artist","provider_id":"<the plugin's artist identifier>"}`. The identifier is the `provider_artist_id` the plugin itself stated in the structured credit of a recording ([credited artists](metadata.md#credited-artists-from-providers)). No new capability is declared, so a listener who approved `artwork` approves nothing new, and a plugin that does not know the kind answers an invalid request, which Hito treats as "no photo" (the credit is marked `missing` and asked again only after a day). The Spotify and YouTube plugins in this repository answer 400 to a body their enum does not accept; a plugin built on the default axum JSON extractor answers 422. Both are covered by the fake plugin. Nothing else about the wire changes; the response is the same validated PNG or JPEG bytes.

Hito asks only for a credit that qualifies:

- the account owns a library recording whose current provider observation names the credit as a main artist with a provider artist identifier, and that observation's connection is active and declares `artwork`;
- **Fetch album artwork automatically** is on (with the choice off nothing is queued and nothing leaves the host, and a stored photo is no longer served);
- the credit has no chosen MusicBrainz identity, or has one whose Wikidata chain already ended `missing`. A chosen identity whose photo is still queued, ready or temporarily unavailable keeps the MusicBrainz path.

The photo uses the same queue as the others. Migration 0081 rebuilds `cover_art_lookups` so its kind check also admits `credited_artist` and its identifier column holds Hito's own artist identifier (1 to 64 letters, digits and hyphens) for that kind; the column is still named `mbid`. The account's credit identifier is never the plugin's identifier and is the only value the browser sees. `GET /api/v1/cover-art/credited-artist/{artist_id}` queues once (202), then answers 200 with the stored image, 404 when the credit does not qualify, there is none or the choice is off, or 502. The worker runs one lookup per second at most. Credited photos have their own limit of 32 pending lookups per account, separate from the 32 for MusicBrainz photos, and both stay inside the 64 pending lookups per account; a stray identifier that is not a qualifying credit queues nothing. A stored photo is served only while the credit still qualifies: if the connection is deactivated, `artwork` is revoked or the recording is removed, the route answers 404 again. If the plugin transport is busy when the lookup runs, the lookup goes back to the queue after five seconds without counting an attempt, so busy time never moves it toward `unavailable`. The library credit list marks qualifying credits with `provider_photo` (only when automatic artwork is on and an active connection declares `artwork`, checked once per page before any per-credit query; the per-credit query searches every table by key), so the web app asks only for those; credit cards and the credit page show the photo and fall back to the album cover after the browser's retries.

Covered by 20 core cases against a fake plugin (a photo requested exactly once and stored; a credit with no provider identifier never queued or requested; the choice off, also between queueing and fetching; `artwork` revoked between queueing and fetching; a plugin that answers 400 or 422; identifiers that are not owned credits; another account that can neither queue nor read; the two caps and the total of 64; a busy transport; the three ways a stored photo stops being served; the MusicBrainz fallback order with a missing, queued, running or unavailable MusicBrainz photo), two upgrade cases for migration 0081 (kinds and lengths; ready, running and unavailable rows kept whole with a clean foreign key check and both partial indexes), one HTTP case and 4 DOM cases. A one-off measurement with 600 credited artists and 3,000 credited recordings listed a page of 100 artists in about 6.5 ms with an active connection and 4.7 ms without one. Not covered: the chosen-artist cards and the chosen-artist page (their identity group is not a single credit, so they still show only the MusicBrainz photo and then the album cover), and any live Spotify request.

The choice **Fetch album artwork automatically** (default on, see [preferences](metadata-preferences.md#automatic-lookups)) governs all of this; turning it off queues nothing and drops pending work. Covered by an owned loopback archive fake (core, client and HTTP cases). The live archive has not been reached from this source. The server code runs on the local development server; this is not a public release.
