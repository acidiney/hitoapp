# Timed lyrics and backing-track contracts

The Apache-2.0 plugin SDK defines structured lyrics and backing-track responses. Existing manifest bytes and capability declarations remain unchanged. The host can read lyrics for a retained, authorized provider identity. Static rendering is qualified below. The additive v2 contract and standalone LRCLIB reference service are qualified below. Independent lyric identities have backend qualification below. Their UI integration, verified audio alignment and backing-track playback remain unfinished.

## Protocol and identities

Services declare the existing `lyrics` or `backing_tracks` capability and expose the corresponding descriptor. The host negotiates the lyrics extension independently. An unsupported version does not disable unrelated approved capabilities. Future backing-track lookup must preserve that rule.

| Method and relative path             | Request or response                              |
| ------------------------------------ | ------------------------------------------------ |
| `GET v1/lyrics/capabilities`         | `{"lyrics_version":1}`                           |
| `POST v1/lyrics`                     | `LyricsRequest` → `LyricsResponse`               |
| `GET v1/backing-tracks/capabilities` | `{"backing_tracks_version":1}`                   |
| `POST v1/backing-tracks`             | `BackingTracksRequest` → `BackingTracksResponse` |

Both POST requests contain `recording`, with the chosen service's opaque recording `id` and nullable `revision`. Both responses must echo that exact reference, even for empty results or unknown revisions. Each ID or present revision must be nonblank, contain no control characters and fit within 256 UTF-8 bytes.

These are provider identities, not Hito IDs or another provider's source identifiers. A lyrics provider can supply its own metadata identity independently of the service supplying audio. Lookup must not disclose local paths, credentials or signed audio URLs.

```mermaid
flowchart LR
  Ref[Exact provider recording reference] --> Lyrics[Lyrics request and validation]
  Ref --> Backings[Backing-track request and validation]
  Lyrics --> Document[Document and rendition timeline]
  Backings --> Recordings[Separate recording candidates]
```

## Lyrics

`LyricsResponse` contains `recording` and nullable `lyrics`. Null is a valid negative answer. A present `TimedLyrics` document contains:

- `id` and `rendition_id`, each bounded to 256 bytes. Document identity and rendition timeline have separate meanings, including when a provider uses the same string in their separate namespaces.
- Nullable `revision`, `rendition_label`, `language` and `duration_ms`. Present revisions have the 256-byte limit; labels have a 2,048-byte limit and language has a 64-byte limit. Unknown values remain unknown.
- Nullable `attribution`, using existing recording credits and a canonical HTTPS backlink without credentials, query or fragment.
- `lines`, containing one to 2,000 objects with integer `start_ms`, integer `end_ms` and plain `text` bounded to 2,048 UTF-8 bytes.

Intervals use milliseconds on the declared rendition timeline. They are half-open, ordered and nonoverlapping: `start_ms < end_ms`, and the next start is at least the previous end. Gaps and touching intervals are valid. No interval may exceed seven days. A known duration must cover every interval and cannot exceed seven days. Negative, fractional and overflowing times are rejected. Text limits apply before trimming or scanning; blank text and control characters are rejected.

The complete encoded response, including recording reference, metadata, attribution, lines and JSON escaping, cannot exceed 262,144 bytes. `MAX_LYRICS_RESPONSE_BYTES` publishes that bound. Validation serializes into a bounded counting writer without allocating another complete response buffer. The HTTP host also bounds received bytes before decoding JSON and applies one total deadline to negotiation and response reading.

## Plain lyrics and start anchors in version 2

The additive extension uses `GET v2/lyrics/capabilities` with `lyrics_version: 2` and `POST v2/lyrics`, reusing the exact existing `LyricsRequest` provider reference. `LyricsResponseV2` returns a nullable `LyricsDocument`. Legacy v1 types and bytes remain unchanged.

A v2 document keeps document/rendition identity and unknown revision, language and duration separate. `content.kind` is `plain`, with text lines, or `anchored`, with ordered integer `start_ms` and text. Plain blank stanza lines, simultaneous anchors and blank clear events are valid. There are no invented line ends. A dedicated optional attribution accepts an empty creator list; a recording performer does not become a lyric author.

The complete encoded response remains capped at 262,144 bytes, one to 2,000 lines and seven days. Raw line bounds and controls are checked before normalization. Exact identity echoes, known-duration coverage and canonical credential-free HTTPS backlinks remain enforced. Anchors belong to the provider rendition and do not establish alignment to Hito audio.

[LRCLIB](plugin-lrclib.md) supplies this extension without audio or sources. Two independent review rounds and a fresh source build pass 20 service tests and all 145 schema-enabled SDK cases, strict checks, production default/schema feature checks and formatting. This is Linux Docker fixture qualification. Host choice/review backend qualification is below; UI work remains separate; the [frontend specification](frontend-lyrics-integration.md) is available for parallel implementation.

## Host lyric lookup

`POST /api/v1/recordings/{id}/lyrics` requires a normal authenticated account and browser request protections. Its JSON body contains `observation_id` and `installation_revision`. The host derives the provider recording reference from that retained observation. Clients cannot supply an arbitrary provider ID or match by title.

The recording must belong to the caller's personal library. Its selected primary observation, or its current attached source-provider observation, must still belong to a live nonretired binding. The installation must belong to that account, remain active, approve `lyrics` and match the requested revision. Historical observations cannot silently substitute for the current provider identity. Session, library grant, binding, observation and installation authority are rechecked during held responses and before publication.

The result has a `status` field:

| Status        | Meaning                                                                                                                          |
| ------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `ready`       | Contains the exact Hito `recording_id`, `observation_id`, host `document_digest` and SDK `document`. `document.lyrics: null` is a valid negative answer. |
| `unsupported` | The approved service does not support version 1 of the lyrics extension. Other approved capabilities remain available.           |
| `unavailable` | The provider failed, returned invalid data, exceeded the received-byte limit or missed the total deadline.                       |
| `cancelled`   | The caller cancelled the public core operation.                                                                                  |

Authorization and stale-revision failures retain their normal HTTP errors. Successful and account-authorized error responses are private and not stored in shared caches.

Negotiation accepts at most 4,096 received bytes. A lyric response accepts at most 262,144 received bytes, including whitespace, before SDK validation. Both exchanges share one ten-second lifetime. Lyrics have separate admission of at most four in-flight requests globally and one per account, so a held lyric response does not consume playback-source capacity. Cancellation or authority loss drops the upstream body and releases admission.

Lookup does not modify the library or collection, adopt a recording, resolve sources, fetch audio, prepare transfers or create a download. Structured completion events contain generated correlation IDs, safe outcomes and elapsed time. Provider identities, document text, revisions, credentials and private paths are excluded.

The legacy lookup serves current retained provider observations. The independent identity workflow below also supports authorized scanned local recordings. Neither workflow establishes audio alignment or enables synchronized highlighting.

## Document fingerprint

Every ready response includes a lowercase SHA-256 `document_digest`. Hito computes it from its serialization of the complete validated typed `LyricsResponse`, after the existing authority recheck. It includes provider reference, document/rendition identities, text, timings and attribution, including a valid null-lyrics response. Clients treat the value as opaque. It identifies returned content; it does not certify its provenance or establish an audio timeline. No registry, persistence, additional provider request or new SDK protocol is introduced, and digests are excluded from workflow logs.

One authenticated HTTP tracer checks identical-response stability and separate timing/text changes with unchanged document IDs and revisions. It also preserves authorization, exact provider calls, private state and original audio, and checks diagnostics before its digest assertions. The initial missing-field failure passes unchanged after the implementation. All fourteen normal lyric cases and scoped core/server strict checks pass; two existing diagnostic cases remain ignored. An independent Docker execution repeats the new case using the retained qualified executable, after matching the combined Rust/Cargo/vendor inputs apart from a service README update. That execution passes in 0.84 seconds and leaves no containers.

Actual Rust export and TypeScript generation add only the required ready-result field, preserving all 105 paths, 257 schema names and other contract content. Both TypeScript configurations pass after adding that field to the shared typed test fixture. Static rendering is unchanged. Reviewed offsets, current audio-load ownership and synchronized highlighting remain separate work.

## Current lyric providers

`GET /api/v1/recordings/{id}/lyrics/providers` returns the caller's currently retained lyric-capable identities. Each item contains its observation, installation and current installation revision, exact provider recording reference and display label. Selected primary identity comes first, followed by current attached source-provider identities in stable order. Historical observations, inactive installations/services and unapproved lyric capabilities are excluded. Independent lyrics-only connections use the separate account-private identity workflow below.

This is a local authorized read with a one-second lifetime and at most nine current identities, derived from the existing binding limit. It never contacts providers, reads audio, prepares a source or creates persistent state. Malformed recording IDs use the existing `422` validation response. Lookup still uses the original POST authority checks; listing choices does not authorize an old observation or installation revision.

Three public cases qualify primary identity, attached current-versus-historical observations, and account/current-approval boundaries. They preserve private owner and peer state and original audio, verify no provider work on listing, and require separate safe completion logs. All 13 normal lyric cases pass, with two existing diagnostics ignored, alongside strict core/server checks and an independent 13-case repeat. Actual Rust export adds only this path and two DTO schemas, preserving all prior 98 paths, 231 schemas and other contract metadata. Actual generated TypeScript and both type configurations pass. The shared chooser and its packaged checkpoint are qualified below.

## Choosing a retained lyric provider

Track Details offers **Choose lyric provider** without requesting choices or lyrics when the page opens. The action reads the current local identities above. Selecting a row makes no lyric request. **Show lyrics** then sends that row's exact observation and installation revision; the host derives and revalidates its provider identity. A local recording can use a current attached provider even when its primary metadata came from a local file.

If the connection changes and lookup rejects the old revision, refreshing the chooser reads current authority again. It preserves the selected observation when still eligible and keeps the unsaved metadata editor intact. It requires another explicit Show before contacting the provider. Historical observations do not supply replacement choices.

```mermaid
flowchart LR
    Choose[Choose lyric provider] --> Current[Local current-identity read]
    Current --> Selection[Explicit provider selection]
    Selection -->|Show lyrics|Lookup[Authorized retained-identity lookup]
    Lookup --> Lines[Static lines and credits]
    Lookup -->|Stale authority|Refresh[Refresh current choices]
    Refresh --> Current
```

The list and lookup have separate account/context request owners. Cancel, account changes, primary-observation changes and selected-provider changes retire the relevant work and reject late bodies. List reads include JSON decoding in a five-second deadline; lyric lookup retains its fifteen-second client deadline. Neither workflow subscribes the page to playback position updates.

Nine new DOM cases plus the six existing lyric cases pass together, including an independent Docker repeat and another repeat after integration. The stale-revision case first reproduced the failed refresh, then passed without changing its assertions. Other cases qualify local-primary selection, cancellation, account/context changes, provider reselection, complete-body deadlines, exact commands, private logs and edit retention. Both TypeScript configurations and the integrated production bundle pass. The read-only tools environment initially rejected Vite's configuration-cache write; the bundle passed with Vite's runner configuration loader and unchanged application inputs.

These controlled-fetch DOM checks and source reviews qualify the shared source. The separate packaged checkpoint below exercises the retained-provider chooser. Live provider discovery, reviewed identity adoption and rendition alignment remain release work.

## Shared static lyric view

Track Details now offers **Show lyrics** for a selected primary provider observation. Clicking it reads the current approved installation revision and starts the retained-identity lookup above. Opening the page does not request lyrics. Hito renders plain lines and structured credits; the plugin supplies no executable interface. A valid null answer, unsupported capability and temporary failure have separate states with explicit retry.

The workflow belongs to the account, recording and selected observation. Cancel, navigation or a context change retires the request and fences late replies. The client also bounds the complete settings/read/JSON lifetime to fifteen seconds. A ready result must echo the current Hito recording, observation and provider reference before it appears. This static view does not subscribe to playback position or claim rendition alignment.

```mermaid
flowchart LR
    Details[Track Details] -->|Explicit Show lyrics|Owner[Account and observation request owner]
    Owner --> Settings[Current approved plugin revision]
    Settings --> Lookup[Authenticated retained-identity lookup]
    Lookup --> Lines[Static lines and structured credits]
    Changed[Cancel or context change] --> Retire[Abort and reject late results]
    Retire --> Owner
```

Six DOM cases exercise the actual page and HTTP client against controlled fetch responses. They check explicit lookup, immutable personal edit drafts, exact request bodies, cancellation, account and observation changes, null and error states, wrong identity echoes, retries and private-log exclusion. The deadline case uses a simulated frontend clock; it does not prove real network timing or upstream body retirement. All 511 frontend cases, both TypeScript configurations and the production bundle pass in an independent Docker repeat. Fresh source review also passes.

Packaged reading, cancellation and retained-provider selection are qualified below with owned provider fixtures. Live provider acceptance, timeline alignment and backing-track UI remain open.

## Cancellation completion

A polled lyric request now owns its completion event and tracing span until the future is dropped. Cancellation restores the original log dispatcher, emits one static outcome with elapsed time and retires the core span before releasing that dispatcher. A future dropped before its first poll emits no event. Authority checks, upstream body ownership and lookup outcomes are unchanged.

The public regression holds an actual provider response, polls under one subscriber and drops under another. It requires peer EOF before fixture release, one correlated cancellation, exactly one core-span and parent-span close event, and an empty decoy capture. A fresh authenticated HTTP retry succeeds without changing private state or original audio. The initial completion regression missed its event; a stronger retirement control later exposed an unclosed span. Each failure passed unchanged after its corresponding ownership fix.

All ten normal lyric cases pass, with two finite diagnostics explicitly ignored. Strict core/server all-target checks, formatting and fresh source review pass. An independent integrated run passes those ten cases and the community-ranking HTTP workflow; actual OpenAPI export remains byte-identical at 98 paths and 231 schemas. The packaged Chromium/WebKit checks below also pass.

Two separate intermittent observations lacked an expected event or span context. Their causes remain open despite finite passing diagnostics. The controlled dropped-future and span-retirement fixes do not establish a cause for those samples.

## Packaged static lyric workflows

Four isolated cases pass against the runtime built from `ee17d7336c8942a6a283d3c59d635fa0f6c2bdfb`: reading and lifecycle in both Chromium and WebKit. They drive first-run setup, plugin installation, Discover adoption and Track Details. Only explicit lookup contacts the owned provider. Exact escaped text, structured credits, a valid null answer and unsaved metadata drafts are checked. No playback, preparation, persistent download or implicit write occurs; private state and original audio remain unchanged.

Each lifecycle case holds two real provider responses. Cancellation and navigation retire their bodies before fixture release, then immediate explicit retries succeed. Host outcomes are exactly `cancelled`, `ready`, `cancelled`, `ready`, with separate correlation IDs and elapsed times. Reading produces exactly `ready`, `not_found`. Both engines report no JavaScript errors, unexpected failed requests or private console values.

The immutable image is `sha256:003cd13c5e575c3bfe77d084772f506fbe4edd32db6e6e16c5419775184e438c`. Its 1,970 committed inputs hash to `b3240208bc22b403f2452afb1cd5ce532d5012bc031f3df05c2672d5af871999`. The separately recorded gate overlay preserves the exact executed test code; later gate and Privacy commits are not part of this runtime. See the [reproduction guide and artifact identities](../scripts/lyrics/recording-browser.md).

These are owned protocol fixtures and Linux browser engines. Account-switch and late-owner browser controls, live providers, physical platforms, timeline alignment and backing playback remain open. The two historical missing-log observations remain unexplained. The normal persistent server was not changed.

## Packaged retained-provider selection

One complete chooser workflow passes in Chromium and WebKit, including an independent repeated run of both engines. It reviews and connects two owned HTTP providers, adopts A through Discover and attaches B through the public generic resolver. It verifies the current retained observations before opening Track Details. No database fixture invents those identities.

Choose lists A and B without contacting either provider. Selecting B makes no lyric request. A separate Show sends B's exact observation and installation revision, returns its opaque provider reference and renders its escaped lines and structured credits. A receives no lookup; B receives exactly one. The selected primary metadata, unsaved editor node and draft, owner/peer state and original WAV bytes/metadata remain unchanged. The chooser makes no audio, preparation, download or implicit-write request.

Both engines pass the host outcome and privacy verifier, source/module pins and exact owned-container cleanup. Local listing and ready lookup each have a distinct completion correlation. Only exact optional missing-resource responses have matched native console diagnostics; unexpected failures remain rejected. The independent two-engine run passes in 24.44 seconds. This elapsed time includes fixture orchestration and is not a release-latency benchmark.

The immutable image is `sha256:d1622a90f01be763b6391f241df935e2f98bac124400ce1ea0331509c854e024`, built from `6cc3a0a42fcbde7bb590bcd0de644a5aa8cefa23`. Its 1,994 product inputs hash to `bff5049f4248acd9a3a495d43adf752cc0f7c08377933b2543ad2ec04c68342b`. Executable gate inputs remain byte-identical after integration; the reproduction README received only its qualification and checkpoint instructions. See the [isolated runner and reproduction guide](../scripts/lyrics/provider-choice/README.md).

A real tool timeout first exposed an unremoved one-off container. Exact project/name/run ownership checks and a `finally` cleanup now pass six failure/success controls, an independent timeout repeat and fresh review. The first browser run passed its UI assertions but rejected equivalent macOS `/var` and `/private/var` bind paths. Canonical path comparison passes that saved inspection while rejecting all 19 altered mount/runtime controls. Both original failures remain recorded; no application behavior changed to resolve them.

This checkpoint covers retained-provider selection with owned protocol fixtures. It does not qualify an arbitrary lyrics-only provider's identity discovery, live providers, physical platforms, stale-authority browser races, timeline alignment or backing playback. The normal persistent server was not changed.

## Backing tracks

`BackingTracksResponse` contains `recording` and `items`, with zero to sixteen existing `ProviderRecording` objects. Normal recording, release and attribution validation applies. Candidates preserve their own identities and unknown metadata.

Candidate IDs must be unique and distinct from the original recording ID, regardless of revision or display metadata. An instrumental is a separate recording, rather than an alternate source of the original. It must not inherit saved progress by matching a title. Responses contain no source objects, stream URLs or plugin-supplied UI. The complete encoded response has the same 262,144-byte limit, exported as `MAX_BACKING_TRACKS_RESPONSE_BYTES` and enforced by the shared bounded writer.

Future adoption and playback must use existing approved-provider, source-resolution, relay and explicit-download owners. Looking up a candidate must not fetch audio or create a persistent copy.

## Required host and player integration

Highlighting requires a host-owned association between the validated document digest, its rendition and an authorized audio timeline. Verified exact-version evidence or an explicit user-reviewed offset can establish it. A matching title, similar duration or arbitrary provider echo of a source ID cannot. For an accepted offset, lyric position is audio position minus the offset. Without accepted alignment, Hito can display static lines but must not claim synchronized highlighting.

A changed source, metadata observation, playback generation or document digest must retire the old alignment. Hito owns rendering and navigation. Only the timed-text component should subscribe to position observations; catalog pages must not rerender on playback ticks. Independent lyric providers must work with local audio and metadata/audio supplied by different services.

Independent-provider identity review has the backend qualification below. Its UI integration, alignment, backing-recording adoption and end-to-end playback remain unfinished. Microphone mixing, vocal removal, scoring and performance recording remain outside this release.

## Verification

Complete Docker SDK suites pass 68 default and 74 schema-enabled cases. Formatting and strict schema-enabled all-target SDK Clippy pass. New public cases first reproduce missing temporal, text, attribution, candidate and encoded-size enforcement before their fixes. They verify exact 262,144-byte boundaries and one-byte excesses, Unicode and JSON escaping, exact nullable version echoes, unknown-field rejection, legacy manifest bytes and schemas derived from actual Rust types.

Independent security, correctness and performance reviews pass. These checks qualify SDK contracts; they do not establish provider compatibility, timeline alignment, UI behavior or platform acceptance.

Nine host HTTP/core cases pass against real temporary SQLite, owned HTTP protocol providers and a read-only original WAV. They cover exact retained identity and null results, unsupported versions, wrong identity echoes, received-byte limits, one total deadline, session revocation, explicit cancellation, peer denial and playback isolation. A held lyric response permits another recording in the same account to refresh its actual provider source and relay an exact WAV byte range. Public collection state, original bytes and metadata remain preserved, and lyric lookup creates no persistent download.

One early concurrent two-case run missed an expected completion event. Later passing runs have not established its cause. Independent repeats of all nine host cases pass. A finite concurrent diagnostic also passes in one author invocation and one independent invocation, retaining the same strict assertions. Those passes do not explain the earlier missed event, so its cause remains open. Strict core/server all-target Clippy, formatting, actual Rust OpenAPI export and generated TypeScript reproduction pass. The contract adds one path and six schemas while preserving all prior 95 path objects and 223 schema objects. The host checks do not establish official-provider compatibility, rendering, timeline alignment or packaged platform acceptance.

## Independent lyric identities

An account can explicitly review a Search, Metadata and Lyrics connection for an authorized recording, including a scanned local WAV in a shared library. Core stores the association privately for that account. Another account with access to the same recording cannot list, read or remove it. No provider binding, metadata observation, release, source, audio, preparation, download or collection entry is created or changed by this workflow.

These routes extend `/api/v1/recordings/{id}/lyrics`. Their request and response definitions come from the [registered Rust routes](../apps/server/src/api/lyric_identities.rs) and [Core wire types](../crates/core/src/catalog/timed_content/identity_types.rs).

| Method and suffix | Purpose |
| --- | --- |
| `GET /services` | List the account's current approved connections locally. |
| `POST /candidates` | Send an explicitly supplied query to one selected installation and revision. |
| `POST /identities/review` | Fetch typed SDK recording metadata and return its review stamp. |
| `POST /identities` | Refetch, check the same review evidence and retain the private association atomically. |
| `GET /identities` | List current retained observations and reviewed identities without provider requests. |
| `DELETE /identities/{identity_id}?revision=…` | Remove the account's reviewed association at its expected revision. |
| `POST /identities/read` | Read the exact selected observation or reviewed identity at the selected installation revision. |

Listing and opening a chooser make no upstream request. Search accepts at most 512 UTF-8 bytes and requests at most twenty candidates. Review uses only the Metadata protocol. Acceptance binds the exact session, library grant, current recording metadata and personal override operations, installation and service revisions, and refetched candidate metadata. The SDK metadata value has its own embedded review schema; it does not reuse the host's existing `RecordingMetadata` view schema.

At most eight independent identities can be retained per account and recording. An exact duplicate returns the existing identity. Unavailable or revoked connections stay visible with `selectable: false`, count toward the limit, and can be removed locally. They cannot be read. Removing and retaining again creates a new identity UUID, so a deleted selector cannot address its replacement.

A reviewed association also retains a missing shared-library recording. Normal missing-recording cleanup reports it as referenced and preserves the private association. After explicit unlink, cleanup can remove the recording if no other reference keeps it. Account erasure removes these private rows before their installation and account parents; it preserves a shared recording and other accounts' associations.

Selected reads negotiate `GET v2/lyrics/capabilities` first. A missing or unimplemented extension, or an unsupported descriptor, permits independent negotiation of the existing v1 format. Invalid JSON, oversized bodies and other upstream failures remain unavailable. A fallback still validates its descriptor, exact provider reference and document; it never receives a new deadline. One dedicated lyric admission covers negotiation and the response body within the original ten-second deadline. Search, review and acceptance use that same dedicated quota, leaving ordinary playback source refresh and audio relay admission available. Descriptor bodies are bounded at 4,096 received bytes and lyric bodies at 262,144 received bytes before typed validation.

The new read result is `ready_v1` or `ready_v2` with the actual selector, provider reference, validated SDK document and host digest. `unsupported`, `unavailable` and `cancelled` are separate results. A genuine valid `lyrics: null` remains a negative answer. Plain v2 lines carry no manufactured times; anchored v2 lines preserve provider anchors and clearing events. None of these associations establishes rendition alignment with local audio. Rendering remains static until separate alignment is accepted.

Authority is checked during held replies and before publication. Session revocation, grant withdrawal and regrant, association deletion and replacement, personal metadata edits, plugin revisions, current PIN configuration and observed PIN locks retire stale work. PIN checks bind the current hash and idle policy. They do not promise durable history for a setup/remove or policy change and reversal completed entirely between observations while the same session is authorized again. Grant epochs, session identities and association UUIDs retain their stronger ABA protection.

Nineteen new HTTP/core cases pass through real temporary SQLite, owned deterministic WAV files and HTTP protocol fixtures. They cover the private shared-recording workflow, approvals, the cap and unavailable entries, duplicate and deletion behavior, held authority, explicit cancellation and dropped HTTP requests with observed peer EOF, one total deadline, malicious bodies, static outcome logging, actual account erasure, missing-recording retention and the public schema collision regression. One case composes Hito with the actual LRCLIB router and an owned fake upstream. Connect and list make no upstream call; explicit search, review, acceptance and read make exactly one search GET and three record GETs, preserving the local recording, sources, private state and original audio.

A fresh combined-source Docker repeat passes all nineteen new host cases and seventy-two affected regressions: seven missing-recording units, three account-erasure plan cases, twelve missing-recording capability cases, nine existing account-deletion cases, seven source-ranking cases, eleven provider-metadata cases, ten relevant catalog-audio cases and thirteen legacy lyric cases. Strict Core/server all-target checks, the production Core check and formatting pass; all 4,112 frozen inputs remain exact. The actual Rust contract adds six path objects and fifteen schemas without changing any existing path or schema. The two pre-existing ignored lyric diagnostics remain ignored. These are Linux Docker fixtures and backend evidence. They do not qualify a new browser workflow, live LRCLIB availability, packaged images, physical platforms, audio alignment or a public release.

Account-deletion performance has a public Core regression with real migrated SQLite and valid peer identities. Before the installation-leading foreign-key index, deletion used 1,700 VM steps beside 64 peer identities and 50,100 beside 4,096. The indexed implementation uses 900 in both cases, preserves every peer association and keeps the parent-deletion probes selective. Two review rounds resolved this index and missing-recording retention. These counts describe the owned fixture, not release latency.

## Automatic ephemeral lyric reads

`POST /api/v1/recordings/{id}/lyrics/automatic` accepts strict empty JSON `{}`. Automatic loading calls it once per track/session when no explicit saved choice takes precedence. This POST authorizes a search using the recording's committed effective artist and title, including that account's personal overrides. Listing services or identities stays local.

Core chooses the first active, approved Search + Metadata + Lyrics connection in the same order as `GET /lyrics/services`. It searches once for at most twenty candidates. Each compatible candidate gets an exact metadata refetch and a validated document read until a timed result wins or the complete page is exhausted. A continuation is `ambiguous`; the operation does not silently choose from an incomplete result set or try a second service.

Automatic confidence requires full normalized title and artist equality. Case, punctuation and whitespace can differ; featured artists and version suffixes remain part of the comparison. Both durations must be known and at least one second. Their difference must be no greater than the smaller of 2,000 milliseconds and 5% of the shorter duration, rounded down to whole milliseconds. Recognized album version qualifiers retain their full normalized context, including live, remix, acoustic, remaster, instrumental and edit labels. Refetched metadata must preserve the exact searched provider ID and revision. Its title, artist, duration, optional musical title and known album version evidence must still agree. Among identities that pass all these checks, provider order breaks ties within the timed and plain tiers. Multiple compatible lyric identities alone no longer mean `ambiguous`. This changes the earlier unique-identity policy; provider rank still cannot admit a musically incompatible candidate.

The result is an `AutomaticLyricsLookup` tagged by `status`. `ready_v1` and `ready_v2` contain `recording_id`, `installation_id`, `installation_revision`, the approved manifest `label`, the actual SDK `provider_recording`, `document_digest` and validated SDK `document`. These are ephemeral reads. They supply no retained selector or review stamp and leave recording metadata, sources, collection state and private lyric associations unchanged. Existing explicit selection and retention APIs keep their meanings and wire shapes.

`no_services`, `insufficient_metadata`, `not_found`, `ambiguous`, `unsupported`, `unavailable` and `cancelled` are separate results. Insufficient committed metadata sends no search. `not_found` covers no confident match or a genuine valid null lyric result; an upstream failure stays `unavailable`. Current authorization and revision failures keep their ordinary HTTP status. A concurrent lyric workflow can return 409.

One dedicated lyric admission and the original ten-second deadline cover the whole scan, including every metadata and document body. Protocol negotiation happens once inside that admitted workflow and is not cached across manual reads, accounts or installation operations. At most twenty metadata reads and twenty document reads follow the single search. Only the first plain document and the current document need remain in memory. The same current session, recording grant, metadata, PIN-policy, installation and service checks run during held replies and before publication. The existing PIN-history limitation above still applies. Explicit cancellation and actual client TCP EOF close provider bodies and release capacity. V2 anchors and plain lines retain their provider values; this read does not establish rendition alignment.

Author qualification uses real temporary SQLite, scanned owned WAVs, the shared `FakePlugin` protocol service and one composed Hito → actual LRCLIB router → owned upstream case. Directed controls cover unavailable versus absent results, strict request policy, ambiguity and version evidence, exact duration boundaries using 1-, 40- and 120-second WAVs, held authority changes, one total deadline, actual TCP cancellation, malformed and streamed oversized bodies, private logs, the four-account global limit and ordinary source/audio independence. The automatic extension adds one path and two schemas. The accepted combined backend also includes playback lease HTTP delivery. Frontend integration, packaged browser acceptance and deployment remain separate gates.

The exact source is integrated in development commits `4f16bf08` and `25e42acb`. An independent fresh Docker repeat passes all 135 named combined-backend cases, including 21 automatic reads, 19 manual lyric identities, 14 legacy lyric cases and 24 LRCLIB service cases. The two existing lyric diagnostics remain ignored. Strict Core/server all-target checks, production Core checking and workspace formatting pass; all 4,201 source and support inputs remain unchanged. The actual combined Rust API adds four paths and four schemas while preserving all 177 prior paths and 424 prior schemas. See [the combined receipt and limits](status.md#2026-11-11-combined-automatic-lyrics-and-playback-lease-backend). This qualifies development backend behavior with deterministic protocol fixtures. It does not qualify the current UI, live-provider availability, deployment or platforms. That checkpoint used the earlier unique-identity policy. The timed-first behavior below supersedes that selection policy in development source; deployment and playback alignment remain open.


### Timed-first automatic selection

Development source at `9b6dbece` returns the first validated timed document among compatible candidates, even when a plain result appears first. Valid v2 anchors and legacy v1 intervals both count as timed. Core keeps the first plain result while examining later candidates and returns it only when the complete bounded page contains no valid timed result. Valid null documents are skipped. An unreadable or invalid later compatible response yields `unavailable`, rather than falsely treating timing as absent. Metadata that proves a candidate is a different recording or version excludes it from selection.

```mermaid
flowchart TD
  Page[Complete bounded search page] --> Next[Next strict musical match]
  Next --> Read[Validate exact metadata and lyric document]
  Read -->|Timed| Timed[Return first timed result]
  Read -->|Plain| Keep[Keep first plain result]
  Read -->|Valid null| Next
  Keep --> Next
  Next -->|Exhausted| Fallback[Return first plain result or not found]
  Read -->|Upstream failure or invalid protocol| Failed[Unavailable]
```

This uses the existing search and document protocols. It adds no `has_synced` field to strict v1 summaries and makes no request to another service. Explicit reviewed selections remain exact and unchanged. Timed candidate badges in the manual chooser need a separately negotiated contract; they are not supplied by this change.

Fresh independent combined-source verification passes 96 named affected cases: 31 automatic, 19 manual identity, 14 legacy lyric and 32 LRCLIB service cases. The two pre-existing diagnostic ignores remain. The 54 earlier host cases are retained with one explicit test-name mapping for the changed identity-count policy; ten automatic cases are added. The older 12 playback lease cases were inspected only as part of the canonical inventory, not rerun by this slice. Public tests use temporary migrated SQLite, scanned owned audio, the shared protocol fixture and the actual LRCLIB router against an owned upstream. They cover provider order, twenty-candidate fallback bounds, malformed timing, later failures, legacy negotiation, all held authority variants, client EOF, the original deadline, peer isolation and exact manual reads.

Fresh independent R1 review passes. Strict Core/server all-target checks, production checks and formatting pass on the same 2,635 combined backend/support inputs. The actual Rust API remains byte-for-byte unchanged. The receipt has SHA-256 `93e7bc3c08b619b2d4816eaaab31bbb25dc76612bb1bf305f6753b472a76ed5c`. Source is integrated; current packaged browser qualification, production deployment and audio alignment remain open. Anchors are provider data, not evidence of audio alignment.
