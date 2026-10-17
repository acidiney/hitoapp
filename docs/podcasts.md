# Podcasts in development

RSS subscriptions, episodes, authorized playback, hourly conditional updates, manual refresh, unsubscribe and explicit downloads are deployed to the local development server. Shared subscription and episode pages use the existing player. The packaged server passes a public-feed workflow in Chromium and WebKit. Native playback, broader recovery, packaged-download acceptance and public-release gates remain unfinished.

## Ownership

`Podcasts` owns private subscriptions, refresh jobs and episode origins inside core. It uses the existing catalog and collection, so an episode has a stable recording ID and can retain personal metadata, likes, progress and playlist occurrences. RSS subscriptions do not create plugin installations.

```mermaid
flowchart LR
    Request[Subscribe or refresh] --> Podcasts
    Podcasts --> Receipt[Durable request receipt]
    Podcasts --> Jobs[Private refresh jobs]
    Due[Authorized hourly refresh] --> Jobs
    Jobs --> HTTP[Validated HTTP destination]
    HTTP --> Response{Feed response}
    Response -->|200| Parser[Bounded RSS parser]
    Response -->|Validated 304| Checked[Schedule next check]
    Parser --> Publish[Atomic episode publication]
    Publish --> Catalog[Existing recording IDs]
    Catalog --> Collection[Personal metadata and activity]
    Catalog --> Grant[Captured playback authority]
    Grant --> Enclosure[Pinned enclosure request]
    Enclosure --> Stream[Shared authorized audio stream]
```

A subscription retrieves feed metadata. It does not open an enclosure or create a persistent download. Publication gives each episode a stable podcast source UUID. Enclosure addresses stay private inside core; public source facts contain an ID and the label "Podcast audio".

Explicit playback captures the current enclosure address, revision, subscription generation and account authority. Later opens and reads check that snapshot. A replaced enclosure invalidates old grants rather than letting them open the new address. Source choices still apply, and suitable local files and saved downloads retain priority.

An episode is identified by its subscription and opaque RSS GUID. Without a GUID, its enclosure address is the fallback identity. Titles and dates do not prove identity. A changed title or enclosure on an existing GUID updates its inherited metadata while retaining its episode and recording IDs. Missing items are retained rather than deleting listening state.

## Subscription controls and HTTP

Unsubscribing hides the subscription and stops its podcast audio authority. It preserves episodes, recording and source IDs, personal edits, likes, progress and playlist occurrences. Queued refreshes are cancelled. A running refresh retains its lease until its worker retires; a new subscription request conflicts while that obsolete work is still running.

Subscribing again with a new request UUID and the same normalized feed address reactivates the existing subscription and queues a fresh refresh. The original account, private library and access grant must still match. A changed generation prevents an old playback grant from becoming valid again. Replaying an earlier subscribe or unsubscribe request returns its original receipt without changing a later subscription cycle. Unsubscribe receipts share the existing transactional request-history budget.

The development server exposes authenticated JSON routes through the shared account extractor. The Rust handlers generate OpenAPI and TypeScript.

| Method | Route                               | Result                             |
| ------ | ----------------------------------- | ---------------------------------- |
| GET    | `/api/v1/podcasts`                  | The account's active subscriptions |
| POST   | `/api/v1/podcasts`                  | A subscription receipt, HTTP 202   |
| GET    | `/api/v1/podcasts/{id}/episodes`    | An authorized continuation page    |
| POST   | `/api/v1/podcasts/{id}/refresh`     | A refresh receipt, HTTP 202        |
| POST   | `/api/v1/podcasts/{id}/unsubscribe` | An immutable acknowledgement       |

Two polling workers belong to the server host and join its shutdown. Episodes use the existing recording-source and audio routes. Feed retrieval starts no audio or persistent download.

The lifecycle workflow passes through unsubscribe, restart, exact receipt replay and reactivation, including exact WAV playback and retained private state. A separate held-request scenario proves reactivation conflicts while the genuine old worker future is pending, then succeeds after that same worker retires. The old audio grant is first checked after reactivation and remains revoked.

The actual TCP server workflow passes account setup, subscription, background episode publication, source discovery and exact WAV delivery. Another account and anonymous requests cannot read the private episodes or audio. The fixture and host join on success or failure. A separate public HTTP case verifies strict inputs, browser-origin protection, all five served OpenAPI operations and immutable command replay across reactivation. This is development evidence, with no claim about UI operation or shutdown during every active network phase.

## Status and saved playback reads

Subscriptions expose pending work, the last failed check, the last successful check time and the archive revision. A failed retry may still be pending, so failure and pending are independent. Failed checks retain the last successful archive. Feed addresses, enclosure addresses, validators and internal job details stay private.

`Collection.saved_playback` reads current saved positions and speeds for at most 200 requested recording IDs. It authenticates empty reads, rejects mixed inaccessible IDs, preserves the first requested order and omits authorized recordings without progress. It checks the session again after the query and has a one-second deadline. It does not search the latest 200 history entries. The authenticated `POST /api/v1/recordings/progress` route exposes this read through the shared account and strict JSON extractors.

The Inertia routes `/podcasts` and `/podcasts/{id}` return generated page contracts. Detail reads authorize the subscription even for partial requests that ask only for shared props. Episode and progress props load only when requested; one batch covers the selected episode page. These reads start no audio or persistent download.

The subscription page preserves an immutable request after an uncertain acknowledgement, polls pending checks with a bounded retry budget and retains the last good list after a read failure. Definitive input rejection permits a corrected feed with a new request UUID. A delayed acknowledgement preserves any newer draft.

The episode page renders its initial episodes and progress without fetching or playing. Resume sends the saved position, while Start over explicitly sends zero. Pagination replaces the shown page only after the new episodes and their saved positions both load. A stale archive cursor offers Load latest episodes. Account changes and unmounts abort reads and ignore late private responses. The current playback-start command restores position; restoring saved speed through that command remains unfinished.

Manual refresh retains its exact request UUID after an uncertain acknowledgement. Status polling refreshes the episode page and saved progress together. Older server-rendered props cannot replace a newer archive, and a completed check with an unchanged revision releases pagination after a pending check. Refresh leaves the shared player alone. Independent reviews accepted these workflows after regressions demonstrated and corrected both stale-state cases.

Unsubscribe retains its immutable command after a lost acknowledgement, including when status already omits the subscription. Navigation waits for the matching receipt. Accepted Like and playlist changes stay accepted without reloading an inactive detail page. The lifecycle cancels its own pending detail reload before sending unsubscribe and retires that read on account changes or unmount. It does not cancel unrelated router work or control the shared player. Two review cycles exposed the late-mutation and already-started-reload orderings; public-page regressions now verify both fixes.

Docker verification passes the public refresh-state and saved-playback cases, all three existing native saved-position cases, ten core library cases and three server podcast cases. Strict core/server all-target Clippy and formatting pass. The frontend passes 242 tests across 77 files, both TypeScript checks and the production build. These are component and HTTP checks, not complete browser/native podcast acceptance. Independent reviews accepted the private reads, subscription and episode workflows, and manual refresh.

The saved-playback work regression grows an analyzed one-record database to 100,000 owned recordings, closes and reopens it, then measures actual public reads with inherited, absent and fresh statistics. Before fixing join order, inherited statistics caused about 900,000 SQLite VM instructions for one recording and interrupted a 200-ID read at ten million. Indexed probes now use about 6,800 instructions for 200 IDs in each statistics state. Single-ID reads fall below the meter's 100-instruction sampling resolution. Exact results, ordering and privacy checks pass. These are query-work measurements, not latency benchmarks.

## Browser verification

A disposable host runs the real core, HTTP runtime and built Inertia UI with temporary SQLite and an owned 45-second PCM WAV. Only its exact RSS fixture origin is allowed. The example adds no production network setting or route. The browser refuses an initialized server before typing fixture credentials.

The guarded Chromium and WebKit workflow verifies subscription through the UI, metadata with zero enclosure requests, actual WAV delivery and advancing media time, uninterrupted navigation, seeking and pause, persisted progress, reload Resume and explicit Start over. It then likes the episode and refreshes the feed through the UI. The updated archive preserves episode, recording and source IDs, the like and exact saved progress. The player stays paused at the same position and source; refresh fetches no audio and creates no persistent download. Both engines use ordinary click gestures without an autoplay override. WebKit classifies its audio request differently; the test still requires its exact media URL, complete WAV length/header, duration and advancing position. This uses a source-mounted development host, not a packaged release image, native output or a physical phone.

The same workflow then unsubscribes through the UI and verifies its matching receipt and navigation to All podcasts. The active list and episode route no longer expose that subscription. Retained source IDs remain present but unready, while the recording's Like and exact progress remain readable. The paused media element keeps its source and position, with no new feed, enclosure, browser-audio or download request. This does not prove revocation during every active browser or native playback phase.

To reproduce from the repository root, use an isolated Compose project and the podcast overlay. Build the tools image, install/build the web app and compile the example first:

```sh
podcast_compose() {
  docker compose -p hito-podcast-e2e -f compose.yaml -f scripts/plugins/compose.tools.yaml -f scripts/podcasts/compose.e2e.yaml "$@"
}
podcast_compose --profile tools build rust
podcast_compose --profile tools run --rm node sh -eu -c 'npm ci && npm run build'
podcast_compose --profile tools run --rm rust cargo build --locked -p hito-server --example podcast_browser
podcast_compose --profile e2e up -d --no-build --wait e2e-server
podcast_compose --profile e2e run --rm browser npm run e2e -- e2e/podcast.spec.ts --browser=chromium
```

Recreate `e2e-server` with `up -d --force-recreate --no-build --wait e2e-server` before running `--browser=webkit`; every run requires a fresh first-run database. The separate explicit-download workflow also requires a fresh host and its own opt-in:

```sh
podcast_compose --profile e2e up -d --force-recreate --no-build --wait e2e-server
podcast_compose --profile e2e run --rm -e HITO_PODCAST_DOWNLOAD_E2E=1 browser npm run e2e -- e2e/podcast-download.spec.ts --browser=chromium
```

Recreate the host again before its WebKit run or before switching workflows; the download workflow stops its owned upstream service. Finish with `podcast_compose --profile e2e down`. The overlay publishes no host ports and mounts no persistent application data.

### Packaged public-feed verification

The opt-in `podcast-live.spec.ts` workflow uses the actual packaged server with fresh tmpfs data and no production network exceptions. On 18 October 2026, Chromium and WebKit passed UI setup, subscription to [Rustacean Station's public feed](https://rustacean-station.org/podcast.rss), actual episode playback, uninterrupted navigation, pause, seek and resume. Both require an actual HTTP 200/206 audio response, advancing media time and the expected source. Navigation retains the same audio element with no pause, emptied, ended or error events; the later intentional UI pause verifies the observer.

The workflow rejects an initialized server before entering disposable credentials. It verifies no Download command, no persistent download intent and no new saved source, blocks external browser requests and checks captured console arguments for the selected metadata, feed address, media URL and password. The server alone retrieves public RSS and media. Scoped server-log checks also exclude the disposable password, feed hostname and selected private path patterns. Every disposable host stopped with exit 0, no OOM kill and no restart. These exclusions do not establish complete production-log privacy.

WebKit initially exposed a status-zero browser event before the actual HTTP responses. A diagnostic run retained the failing HTTP assertion and observed subsequent 200/206 delivery with advancing audio from the same source and no media error. The final wait excludes only non-HTTP status zero; actual HTTP errors still fail, and zero-only playback times out. One later run exceeded the initial media-advancement deadline; its cause remains unconfirmed. A subsequent run passed the unchanged playback conditions after adding safe boolean diagnostics. This is one public-feed compatibility check, not codec coverage, a startup reliability result or a release latency benchmark.

Use an existing packaged image built from the matching application source and the installed browser dependencies:

```sh
export HITO_PODCAST_E2E_IMAGE=sha256:976ad6692811c8b14633b125c3e5ec015b1f8dd9ae06d0ef3c6906298ab2ff11
export HITO_PODCAST_LIVE_E2E=1
podcast_live() {
  docker compose -p hito-podcast-live -f compose.yaml -f scripts/podcasts/compose.live.e2e.yaml --profile e2e "$@"
}
podcast_live up -d --wait --no-build --pull never e2e-server
podcast_live run --rm --no-deps --pull never browser npm run e2e -- e2e/podcast-live.spec.ts --browser=chromium
podcast_live down
podcast_live up -d --wait --no-build --pull never e2e-server
podcast_live run --rm --no-deps --pull never browser npm run e2e -- e2e/podcast-live.spec.ts --browser=webkit
podcast_live down
```

The overlay publishes no ports and mounts no persistent application library or music folder. Each engine requires a new first-run server. `HITO_PODCAST_LIVE_FEED` optionally selects another canonical public HTTPS RSS feed. This live check needs external availability; the owned fixtures above remain the reproducible behavior checks.

## Scheduled and conditional updates

Successful feed checks schedule the next update one hour later. The public worker admits due subscriptions inside its existing claim transaction. Automatic checks create no client request receipts and retain the existing limits of 16 pending jobs per account, 128 globally, one running per account and two globally.

Core stores validated ETag and Last-Modified values with the exact normalized address of the successful representation. Conditional headers go only to that address, including across redirects. A 304 response requires a previous complete publication and a conditional request actually sent to the final address. It advances the successful-check time and schedule without publishing episodes or changing archive cursors, source identities or playback grants. A 200 response replaces validators, clearing omitted values. A valid 304 retains missing values. Malformed, duplicate or oversized optional validators are ignored.

Only a successful, authorized transaction may retain a new fetch address from an entirely permanent redirect chain. The original feed identity remains unchanged. Failed retrieval, parsing or publication preserves the last good snapshot. Recoverable publication errors roll back a savepoint before the worker records failure and releases its lease. Ordinary retries wait two and four seconds; a third failure or exhausted recovered lease postpones the next automatic job by one hour.

Cleanup keeps the newest 32 terminal jobs without client receipts for each subscription. Jobs referenced by immutable subscribe or refresh receipts remain retained. Both receipt lookups have dedicated indexes. This cleanup changes refresh bookkeeping, not listening history or personal data.

The public due-check fixture reopens core, ages only the due timestamp and verifies the actual conditional request and accepted 304. Its old cursor and captured grant still work, including exact WAV delivery. A real same-origin redirect to another path receives no old validators; its unsolicited 304 is rejected without replacing the last successful feed state. Another fixture performs 36 automatic checks after an initial publication and explicit refresh. Exactly the newest 32 automatic jobs and both receipt-backed jobs survive, with unchanged private metadata, duplicate playlist entries, progress, activity, synchronization events and exact receipt replay. These tests advance fixture timestamps; they do not prove an hour of wall-clock scheduling.

The history fixture crosses observed SQLite clock boundaries before the manual 304 and before receipt replay. It requires the successful check time to advance, automatic checks never to regress it, and old receipts to preserve the final time without issuing HTTP requests. Every other subscription field and the full published/private snapshot remain equal. This corrects an earlier clock-dependent assertion that included the intentionally changing check time; controlled clock advancement reproduced that failure in both current source and the earlier accepted baseline. No production refresh behavior changed.

## Network and parsing

The HTTP implementation validates every resolved address and pins connections to that snapshot. Each redirect passes the same checks. Private destinations require an exact origin in administrator-controlled `CoreConfig.podcast_network`; ordinary subscription requests cannot supply an exception. No runtime environment setting or settings UI exists for this configuration yet.

The transport disables proxies, cookies, automatic redirects and retries. It uses fixed request headers, separate DNS/connect limits and a deadline for obtaining headers. Feed retrieval has a 20-second total limit and an 8 MiB body limit. Enclosure requests reuse the validated transport, with a 10-second header deadline and a 15-second limit on each awaited body read. There is no whole-audio deadline. Podcast audio allows 16 simultaneous streams and two per account, separate from the existing provider audio limits. Permits stay with the stream until completion, error, drop or revocation.

Podcast and provider audio share the strict representation parser, bounded buffering, revocation monitor and stream framing. The monitor can close an unread connection and release its permits after authority changes. It retains the tracing span and subscriber for safe deferred diagnostics.

The current parser accepts UTF-8 RSS 2.0, including iTunes author and duration fields. It treats descriptions as text and rejects DTDs, unknown entity references, malformed XML and duplicate episode identities. Its bounds include 64 levels of XML nesting and 5,000 audio episodes. Other feed formats and encodings have not been accepted.

## Verified so far

The conditional-refresh checkpoint on 17 October 2026 passed 26 core cases and three server cases. Formatting and strict core/server all-target Clippy pass. The HTTP milestone also regenerated OpenAPI and TypeScript and passed the frontend production build. Separate independent reviews of lifecycle/HTTP and scheduled/conditional production changes each passed their first round. These are selected checks, not a full-suite or release result.

The earlier core-media checkpoint passed 41 core cases and one production-process logging case, including 20 existing playback/provider compatibility cases. Its coverage overlaps the current checks and must not be added to their count.

Foundation checks cover opaque GUIDs, relative enclosure addresses, empty optional feed fields, XML name validation, rejected external and undeclared entities, declared and unique expanded attribute names, actual pinned TCP delivery, blocked private redirects, mixed DNS rejection and rejection of empty userinfo before a connection or subscription is persisted.

The public scenarios use temporary SQLite and owned HTTP feeds. They verify restart durability, exact subscription and refresh receipt replay, stable episode/recording IDs after a refresh, and preservation of personal titles, likes, progress, playback speed and duplicate playlist occurrence IDs. Artist discovery follows inherited author changes while preserving personal overrides. Out-of-range duration and byte-length values preserve the last good snapshot and release the worker so another feed can proceed. Metadata-only scenarios never retrieve an enclosure or create a download intent.

The media scenarios verify exact deterministic WAV playback through a captured grant and HEAD, bounded ranges, suffix ranges and truthful 416 responses through `Catalog.open_audio`, the server's core entry point. Actual upstream requests contain only the expected safe headers. A replacement-enclosure scenario retains two old unread bodies, observes account saturation, then opens two fresh bodies before touching the old ones. Both new bodies return the replacement WAV; old grants and bodies reject further access without network I/O. Source identity and personal state survive. None of these operations creates a persistent download.

The logging regression launches the real server with a temporary database and `RUST_LOG=trace`, then contacts an owned HTTP plugin. It verifies private hostnames, credentials and paths stay out of logs while safe application DEBUG events and HTTP correlation remain available. Server and desktop initialization share the hard target filter. The desktop path has no equivalent process-level privacy evidence yet.

The request-work case uses 60,003 terminal jobs and 60,003 retained request receipts. Indexed job lookup and transactional receipt counters reduce measured new subscription work from about 840,000 to between 100 and 300 SQLite VM instructions, and refresh work from about 660,200 to between 200 and 300. Exact replays stay below 200 instructions at the meter's 100-instruction resolution.

Archive browsing uses a retained episode count and indexed continuation queries. Pages order dated episodes first, by publication time and episode ID descending, followed by undated episodes in descending ID order. Continuation cursors belong to one account, subscription and content revision; a successful 200 publication requires browsing to restart, while an accepted 304 retains the cursor. A 100,000-episode fixture with repeated dates and 40,000 undated entries reaches deep pages through the public API. Twenty-row pages at depths 0, 59,998 and 99,980 used about 2,200 to 2,900 SQLite VM instructions in the core-media checkpoint, compared with about 602,500, 962,600 and 1,202,000 before the change. Exact episode and recording order, total counts and the terminal page are verified. These are query-work measurements, not latency benchmarks.

A separate public scenario verifies that refreshing the same GUIDs invalidates an old cursor while retaining episode IDs, total count and personal state. A fresh continuation works with a changed page size. Basic malformed-cursor, foreign-account and wrong-subscription reads are rejected. More malformed payload and concurrent authority boundaries remain unverified.

Personal-library creation belongs to identity and access management. RSS, provider adoption, playlist copies and Follow reuse that private library operation. Six existing provider, playlist-copy and Follow scenarios pass after moving the helper without changing its SQL, labels or ownership behavior. Strict Clippy passes for all core targets.

These checks do not establish the public release gates. The populated full-migration fixture reaches 0044 and preserves its prior data checks; the historical reset migration remains tested through 0039. A separate closed/reopened 0040-to-0041 database verifies typed old rows, rowids, generated sort keys, schema and migration checksums, private metadata/activity and duplicate playlist occurrences, then reopens through the latest core migration. It accepts only the intended additive validator column in the original table definition, verifies NULL legacy values and checks both exact receipt-index definitions. Exactly two new podcast source UUIDs remain stable across two core restarts, and an existing unavailable local source stays intact. This is a migration fixture, not a deployment backup rehearsal.

Podcast-specific pending-handshake/read cancellation, logout, original-grant replacement, mixed manual source choices, hostile framing/redirect races, candidate iteration and global stream saturation still need broader public coverage. Existing provider cases exercise several shared mechanisms, but do not prove those podcast paths. Crash recovery, retained-episode removal, native output and release performance also remain open.

## Explicit downloads

Only an explicit Download request creates a persistent episode copy. Subscription, feed refresh and playback do not create one. Podcast and provider transfers share the existing private download intents, worker limits, managed-file verification and removal workflow.

A podcast intent captures the episode and source IDs, subscription generation, enclosure revision and address, account, access grant and backend host. Its worker checks that durable authority rather than retaining the requesting browser session. Logout therefore does not cancel an accepted download. Unsubscribe or enclosure replacement revokes unfinished work. Ready copies use retained-file authority separately.

The owned-WAV public fixture verifies admission, logout, completion, core restart, exact receipt replay and exact local playback after the upstream server has stopped. Another fixture unsubscribes before a queued transfer starts and verifies `authority_changed`, no enclosure request and no reactivation through receipt replay. Both preserve personal metadata, likes, progress and user-owned original files.

A separate public core fixture completes a download before unsubscribe. The same ready intent and managed playback grant remain authorized after unsubscribe and joined upstream shutdown, while the old remote grant is revoked. Local playback returns the exact WAV without another upstream request. Receipt replay preserves the original intent without reactivating the subscription. The recording, personal title, like, saved progress and user-owned original remain intact.

A held-stream fixture delivers and persists exactly 64 WAV bytes, then waits before sending the tail. Unsubscribe commits while that tail remains held. The actual download worker retires within three seconds, closes the upstream body and records `authority_changed` without publishing a copy. Exact receipt replay creates no further request or work. The queued and held-stream cases pass together in 2.01 seconds. The body owns its stream state; fixture shutdown releases the gate and joins its server.

Migration 0044 adds typed podcast origins while retaining provider origins. A closed version-43 fixture checks old typed rows, rowids, sequences, receipts, indexes, migration checksums and ownership views after upgrade. The download checkpoint passed the full default core suite, workspace formatting and strict core all-target Clippy. Three independent source reviews covered security, correctness and performance. This records that checkpoint, not a result for a later integrated full-core run.

The later integrated core run passed 292 distinct cases with no failures, including provider persistence admission and the corrected history fixture. One filesystem-stall injection case remains reserved for its separate flagged run. Strict all-target workspace Clippy, reproducible generated contracts, 242 frontend cases, both TypeScript configurations and the production web build also pass. These are default-feature development checks, not complete native or release acceptance.

Verification of the remaining default workspace targets exposed three maintenance fixtures that still expected the old download-intent columns. The corrected shared fixture accepts exactly migration 0044's seven added columns and verifies every legacy row retains its rowid, text `provider` origin and six NULL podcast fields. Existing typed values, migration ledger, sequences, receipts and file-preservation checks remain intact. All eleven cases across the three maintenance targets pass on rerun; the other default workspace targets passed in the completed run. Final strict all-target workspace Clippy and formatting pass. The live release-image YouTube AAC/native-decoding case remains explicitly opt-in, and feature-gated native checks require their separate runs.

Episode Details uses the shared recording Download component. No episode-row polling was added. The existing component cancels reads on navigation/account changes and polls active transfers. The explicit-download browser workflow passed Chromium in 18.1 seconds and WebKit in 17.5 seconds on separate fresh hosts; both hosts stopped cleanly. It subscribes through the UI, follows Details, clicks Download and waits for Ready without starting playback. UI unsubscribe preserves the ready intent. An example-only POST then joins the owned upstream server and verifies its listener refuses a new connection before reporting it closed. Library playback uses the managed source with advancing media time, and a public audio read returns the exact owned WAV with no further enclosure request.

The same browser workflow removes the download through the UI and verifies HTTP 204, then HTTP 404 for its status and managed audio. The managed source disappears from public source facts, while the retained podcast source remains unready. The recording stays in the library with its personal metadata, like and saved progress intact. A separately ingested user-owned original retains its metadata, availability and exact audio bytes. These HTTP checks prove access revocation, not physical disk reclamation or shared-reference handling. Browser console canaries pass, but do not establish privacy for all server logs or native metadata.

Crash recovery, physical reclamation and shared-reference removal, native playback and downloads, packaged-download behavior and broader deployment acceptance remain open. These development fixtures do not complete the release gates.

## Candidate image and restored-volume rehearsal

On 18 October 2026, the `linux/arm64` server image built from application source `16f009c` passed a restored-volume rehearsal. The normal server stopped for a fresh whole-volume backup and restarted on its existing image. The archive was verified and restored into a new labeled volume, preserving file bytes, permissions, ownership and timestamps before migration. The candidate migration-only command then applied exactly versions 40–44 to that restored version-39 library, without starting HTTP or application workers.

The stopped audit preserved typed rows and rowids across all 97 prior tables, sequences, receipts, keys, host identity and every non-SQLite volume entry. It checked the exact source-defined schema and backfills, nine new tables, two new views, all 44 migration checksums, foreign keys, database integrity and six FTS indexes. Independent origin checks require every legacy intent to retain its original values and rowid plus the text `provider` discriminator and six NULL podcast fields.

The candidate then ran separately with the restored volume, original read-only music bind, matching runtime configuration, no published ports and no network. Health, generated OpenAPI, exact image JS/CSS and anonymous player rejection passed. It stopped with exit 0, no OOM kill and no restart. A separate stopped runtime audit found no logical database changes, managed-file additions, changes or removals, or changes to other volume files and identity. This audit covers the data volume; it does not establish fresh byte checks for every file on external music binds.

The private audit adaptation ran 37 owned probes: 36 passed and one STAT4 case was skipped because the Python engine lacks that extension. Rejection of unsupported expectations is tested separately. Those probes use actual populated version-39 SQLite fixtures and migrations 40–44, while Docker and HTTP control calls are mocked. Three independent source reviews accepted the audit delta. The subsequent archive, restoration, migration and runtime results above came from actual containers and the copied library. The five ordered deployment overlays also pass configuration preflight. The rehearsal itself did not upgrade the normal library.

### Local server upgrade

After the packaged public-feed checks, a new stopped whole-volume backup preceded the normal migration-only command. It applied exactly versions 40–44 and passed the same 97-table preservation audit. The candidate then started without published ports, passed its runtime checks, stopped with exit 0 and passed a separate unchanged-runtime checkpoint. Database rows and managed files did not change during that checkpoint; other volume files, keys and host identity remained intact.

A preflight bound to that stopped candidate preserved all five ordered configuration overlays before restoring the original published ports. The running normal server uses image `sha256:976ad6692811c8b14633b125c3e5ec015b1f8dd9ae06d0ef3c6906298ab2ff11` on migration 0044. Generated OpenAPI, exact image JS/CSS, health and anonymous player rejection pass. LAN and both loopback aliases are reachable. No normal listener account was authenticated or edited, and the folder, Spotify and YouTube containers were unchanged.

The rehearsal archive is `backups/hito-before-source-choice-clone-20261017T231142.967770Z.tar.gz`; the final normal backup is `backups/hito-before-source-choice-normal-20261017T233509.956093Z.tar.gz`. Both retain private mode 0600. The default local server image alias now points to the accepted image, with the preceding image retained separately. This is a local development deployment, not a completed public release.

## Remaining integration

The next work includes broader failure recovery, complete shared UI workflows, native playback/resume and packaged-download acceptance. Status, saved-position reads, Inertia pages, manual refresh, unsubscribe and durable podcast download origins are deployed locally; full podcast acceptance remains open. Startup reliability, native recovery and platform verification remain public-release requirements. See [the release checklist](../TODO.md).

Admission saturation, exhausted-lease recovery, missing-validator responses, publication rollback and cancellation during publication still need direct broader coverage. Passing conditional and retention fixtures does not establish those boundaries.
