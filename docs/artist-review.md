# Artist discovery and explicit review

The core can search and preview artist candidates for a current recording credit, then retain an explicit user confirmation. Same-name candidates keep their provider identifiers. Search and preview do not associate artists, rewrite metadata, merge recordings or fetch audio. Authenticated HTTP routes and recording-detail review controls are verified in source. This feature is not deployed on the normal server.

## Ownership and request flow

```mermaid
flowchart LR
    Credit[Current recording credit] --> Context[Capture account and authority context]
    Context --> Search[Search selected metadata provider]
    Search --> Preview[Preview one exact provider artist]
    Preview --> Confirm[Explicit user confirmation]
    Confirm --> Writer[Recheck context and retain evidence in one transaction]
    Search --> Provider[Built-in or approved personal HTTP catalog]
    Preview --> Provider
    Writer --> Evidence[Immutable recording-credit association]
```

`ArtistCatalog` owns four public workflows: `begin_artist_review`, `search_artist_candidates_until`, `preview_artist_candidate_until` and `confirm_artist_review`. SQLite, permission checks and association writes remain private to core. The existing association API reuses the same private retention code.

The context captures the account, recording, library, local artist and current credited name. It also captures the accepted personal artist operation, source and observation, library-access grant, primary provider binding revision for an import without audio, metadata policy and exact personal connection/service revisions. Reads recheck these facts around provider requests. Confirmation rechecks them inside the same `BEGIN IMMEDIATE` transaction that writes or retrieves the association.

Changing a personal artist from A to B and back to A does not restore an earlier context. The accepted operation identity still changes. A stale search or confirmation fails until the caller captures the current credit again.

## Providers, cancellation and evidence

Search negotiates a separate discovery descriptor. Preview uses the existing profile descriptor, so an older provider can retain profile support without implementing search. HTTP providers receive only SDK request fields, never the local review context. Search pages have at most 20 candidates and an encoded 64 KiB limit; descriptors have a 4 KiB limit. Exact query and snapshot continuation remain validated.

Metadata operations use independent admission, with four global requests and one per account, and a 15-second aggregate budget. They do not consume playback-source control admission. Canceling a search or preview drops its owned request future and response body. Confirmation performs no provider HTTP and holds only a short local write transaction.

Confirmation records `user_confirmed` evidence for the selected recording credit and provider identifier. Repeating the same current confirmation returns the same association. This does not establish a canonical artist selection, guarantee provider availability, merge same-name artists or revise other recordings. Historical associations remain evidence and must not become an implicit current selection.

Workflow events record generated correlation IDs, outcomes, counts and timings. They omit search terms, credited names, private observations, credentials, paths and provider addresses.

## Candidate photos and facts

Each candidate row has a round photo on the left, then the name, one line such as "Person · United Kingdom · 2008–" (type, country name and the year the life span began, each only when MusicBrainz states it; the web app turns the MusicBrainz country code into its English name and shows the raw code if the name is unknown) and the disambiguation. The built-in provider fills `country` and `begin_year` from the search answer, which already carries them; both fields are optional in the shared SDK type and absent from the wire when empty, so personal providers and their earlier bytes are unchanged.

The photo comes from `GET /api/v1/cover-art/artist/{mbid}` ([how it is fetched](plugin-artwork.md#artist-photos)). Only candidates of the built-in provider ask for it, because only those identifiers are MusicBrainz identifiers. A candidate with no photo, a photo still being fetched, an exhausted retry (three tries in all, the retries 2 s and 5 s after the previous failure) or automatic artwork switched off shows the round fallback cover with the artist's initials, never an empty circle. The chosen-artist cards and the chosen-artist page header use the same photo for built-in identities and fall back to the library artwork and then the initials.

## Known releases per candidate

A MusicBrainz artist search does not list releases, so the answer cannot come with the candidates. Each candidate row has a "Show known releases" button instead. Only when the owner presses it, the web app calls `POST /api/v1/recordings/{id}/artist-review/releases` with the review context and that one provider artist identifier. Searching alone makes no release request, and one press makes exactly one request.

Core rechecks the review context as it does for a preview, then reads the first three release groups of that artist through the provider's existing discography call (a bounded browse with limit 3, paced by the shared MusicBrainz client). It returns up to three titles and the same context. A personal provider is asked the same way only when it supports discography; otherwise the answer is `unsupported`. The route writes nothing and has the same `available`, `disabled`, `unsupported`, `unavailable` and `cancelled` answers as preview. A failed request shows a short message and a "Try again" button.

Covered by two core cases (three titles from five with one request, and the unavailable, invalid identifier and disabled answers), one HTTP case and three DOM cases.

## Verified source and remaining work

The full core run passes 331 distinct cases, with the usual filesystem fault-injection case ignored. The 332 successful executions include one nested child execution. Strict all-target core Clippy and formatting pass. Independent security, correctness and performance reviews pass.

Five new public capability cases cover metadata-only A,B,A imports, distinct same-name candidates, nonmutating preview, selection of the second provider identifier, isolated confirmation and idempotent replay. Separate search and confirmation tests reject an A-to-B-to-A stale context. A held search body retires before the provider releases it; the same account refreshes and reads another ready WAV source while search remains pending, then retries successfully. Exact private pins, duplicate playlist entries, likes, progress, metadata, sources and download absence remain preserved. Nine existing profile/association cases pass again after the retention refactor.

## HTTP and recording-detail review

Six authenticated routes expose review context, search, preview, confirmation, retained associations and one exact association profile. They use the shared account extractor and generated request types. Responses are private and not cacheable. Core retains authority, provider admission and transaction ownership.

The shared recording page loads provider preferences and retained associations only after the user opens artist information. Search keeps same-name candidates separate. Preview makes no association; confirmation is explicit. The workflow preserves an unsaved metadata draft and fences late results after account, recording or credited-artist changes. A fresh server context must match the artist evidence actually rendered on the page before it can be confirmed. Uncertain confirmation can be retried without replacing accepted evidence.

Three public HTTP cases cover the workflow, account/origin/context boundaries and a real client disconnect while the upstream response is held. That disconnect retires the upstream request before its release, and a subsequent request succeeds. All 151 server cases, strict all-target workspace Clippy and formatting pass on the combined source. Generated OpenAPI and TypeScript reproduce. All 308 frontend cases, both TypeScript configurations and the production build pass. Independent security, correctness and performance reviews pass after correcting the rendered-credit mismatch with two regressions.

## Packaged browser verification

Image `sha256:79a1e5848641acbed2d3c6ba6cbdd1e173c888c08a5b27744b441053d0a484b1`, built from `e29cd41`, passes the complete workflow in fresh disposable Chromium and WebKit sessions. The browser offers an owned HTTP catalog by address, approves its personal connection, replaces the built-in metadata selection, searches two namesakes, previews the second opaque identifier and explicitly confirms it. Reload preserves the retained review. An unsaved track-title draft survives search, preview and confirmation.

The workflow preserves original WAV hashes, duplicate playlist occurrences, personal metadata, likes, progress, source pins, metadata preferences and plugin connections. It verifies ten exact provider protocol requests and seven correlated frontend workflow completions. Audio remains paused without a source; no source writes, persistent downloads, foreign browser requests or private console values are observed. Separate private backend-log checks also pass. Both TypeScript configurations pass for these browser fixtures, and an independent review of the gate passes.

The first observer incorrectly counted the recording page's two existing read-only download-status requests as downloads. A diagnostic run established their exact authenticated GET envelopes and bounded 404 absence replies. The final gate accepts only those two requests and replies; other download requests still fail. The response-size assertions inspect collected bodies and do not qualify streaming memory bounds.

This package uses an owned catalog fixture. Live MusicBrainz acceptance, normal-server publication, discography and selected-edition track traversal, canonical matching, duplicate-artist correction and artist artwork remain separate work.

## Matched provenance in saved reviews

A saved artist review carries an `evidence` value: `user_confirmed` for a review the account made itself, or the way an automatic match was made (`matched_file_tag_artist_id`, `matched_isrc`, `matched_provider_url`, `matched_name_discography`, `matched_credit_evidence`, `matched_credit_coverage`, or `user_confirmed_candidate` for a candidate the account confirmed in the review queue; see [artist matching](artist-matching.md)). The associations route returns it and the saved-review list on the recording page shows one plain sentence per automatic match, for example "Matched automatically from the recording's ISRC." (a coverage match says "Matched from 85 of 106 tracks of this artist.") A review the account confirmed itself shows no such sentence; a confirmed review-queue candidate says "You confirmed this artist from the candidates Hito found." The wording lives in one place in the web app (`associationEvidenceText`). Covered by an HTTP case (the `matched_isrc` association) and four DOM cases (one per match kind plus a self-confirmed review).

## Searching from the review queue

A credit that Hito could not identify and has no candidates for appears in the Artists page's review queue as "No match found automatically" with a **Search** link. The link opens the recording page of one of its undecided tracks with `?review=artist`. The Artist information panel is then open at "Choose the artist" with the search field already holding the credit name. Nothing is searched until the owner presses **Search artists**, and the rest of the flow (preview, explicit confirmation, **Choose this reviewed artist**) is the one described above. See [the review queue](artist-matching.md#credits-the-walk-left-undecided-without-candidates).
