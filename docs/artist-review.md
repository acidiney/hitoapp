# Artist discovery and explicit review

The core can search and preview artist candidates for a current recording credit, then retain an explicit user confirmation. Same-name candidates keep their provider identifiers. Search and preview do not associate artists, rewrite metadata, merge recordings or fetch audio. HTTP routes and shared review pages remain unfinished, and this source is not deployed on the normal server.

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

`Catalog` owns four public workflows: `begin_artist_review`, `search_artist_candidates_until`, `preview_artist_candidate_until` and `confirm_artist_review`. SQLite, permission checks and association writes remain private to core. The existing association API reuses the same private retention code.

The context captures the account, recording, library, local artist and current credited name. It also captures the accepted personal artist operation, source and observation, library-access grant, primary provider binding revision for an import without audio, metadata policy and exact personal connection/service revisions. Reads recheck these facts around provider requests. Confirmation rechecks them inside the same `BEGIN IMMEDIATE` transaction that writes or retrieves the association.

Changing a personal artist from A to B and back to A does not restore an earlier context. The accepted operation identity still changes. A stale search or confirmation fails until the caller captures the current credit again.

## Providers, cancellation and evidence

Search negotiates a separate discovery descriptor. Preview uses the existing profile descriptor, so an older provider can retain profile support without implementing search. HTTP providers receive only SDK request fields, never the local review context. Search pages have at most 20 candidates and an encoded 64 KiB limit; descriptors have a 4 KiB limit. Exact query and snapshot continuation remain validated.

Metadata operations use independent admission, with four global requests and one per account, and a 15-second aggregate budget. They do not consume playback-source control admission. Canceling a search or preview drops its owned request future and response body. Confirmation performs no provider HTTP and holds only a short local write transaction.

Confirmation records `user_confirmed` evidence for the selected recording credit and provider identifier. Repeating the same current confirmation returns the same association. This does not establish a canonical artist selection, guarantee provider availability, merge same-name artists or revise other recordings. Historical associations remain evidence and must not become an implicit current selection.

Workflow events record generated correlation IDs, outcomes, counts and timings. They omit search terms, credited names, private observations, credentials, paths and provider addresses.

## Verified source and remaining work

The full core run passes 331 distinct cases, with the usual filesystem fault-injection case ignored. The 332 successful executions include one nested child execution. Strict all-target core Clippy and formatting pass. Independent security, correctness and performance reviews pass.

Five new public capability cases cover metadata-only A,B,A imports, distinct same-name candidates, nonmutating preview, selection of the second provider identifier, isolated confirmation and idempotent replay. Separate search and confirmation tests reject an A-to-B-to-A stale context. A held search body retires before the provider releases it; the same account refreshes and reads another ready WAV source while search remains pending, then retries successfully. Exact private pins, duplicate playlist entries, likes, progress, metadata, sources and download absence remain preserved. Nine existing profile/association cases pass again after the retention refactor.

This establishes core behavior, not HTTP cancellation or a completed public release. Recording-detail review pages, hosted disconnect tests, live MusicBrainz integration, discography and selected-edition track traversal, canonical matching, duplicate-artist correction and artist catalog artwork remain separate work.
