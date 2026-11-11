# Independent lyric providers: frontend integration

This specification is ready for parallel frontend implementation by Opus. Backend owns provider requests, authority, identity review, persistence and protocol negotiation. Frontend owns the existing Track Details Lyrics experience and its generic chooser. Plugins supply data, never components or executable UI.

## Current evidence and integration status

> Candidate schema correction, 11 November: do not generate new types from the original OpenAPI SHA `9533036a4b6cbd7300ec86d5989e288900627e697183bd42236d981a0b6ec266`. Its `RecordingLyricReview.candidate` incorrectly references Hito's existing `RecordingMetadata` component because the SDK type shares that schema name. The actual wire response and this specification remain correct: use `candidate.recording`, with optional `musical_title`, `album_title` and `source_ids`. Backend is correcting the field schema and will replace the export and pin below. Endpoints, selectors and UI flow are unchanged, so those can proceed; wait for the corrected export for type generation. Do not work around this with handwritten DTOs or casts.

On 11 November 2026, the separate backend worktree has passed the local scanned-WAV workflow, identity lifecycle, private per-account limit, v1/v2 response validation, held-authority checks, cancellation and deadline controls. A reproduced failure where lyric metadata occupied audio's provider slot now passes with dedicated lyric admission. The LRCLIB service has completed its author tests and is frozen for independent review. The composed Hito → actual LRCLIB service → controlled upstream case, independent final checks and integration are still pending. None of these endpoints is promised available in the current running server yet.

The actual Rust contract is in `/Users/acidineydias/labs/personal/hito-lyric-provider-identities-20261111/crates/core/src/catalog/timed_content/identity_types.rs` and `apps/server/src/api/lyric_identities.rs`. The SDK v2 types are in `crates/plugin-sdk/src/lyrics_v2.rs` in that worktree. Paths, operation IDs and the wire shapes below are agreed for frontend work. Backend must notify Opus if review requires a contract change.

The actual Rust OpenAPI candidate is available now at `/tmp/hito-lyric-provider-identities-20261111/frontend-openapi-current.json`, SHA-256 `9533036a4b6cbd7300ec86d5989e288900627e697183bd42236d981a0b6ec266`. Its source manifest is `frontend-openapi-current-source.json` beside it. The export and all seven operation IDs were verified against the actual Rust routes. Generate types from the supplied actual export, never maintain handwritten DTO interfaces. Local generation against that candidate is fine for implementation and mocks. The final committed `apps/web/src/generated.ts` must come from the accepted canonical `docs/openapi.json`; coordinate that final synchronization with backend. Design, workflow hooks and DOM tests need not wait for backend acceptance.

## User flow

1. Opening Lyrics lists saved identities locally. It may also list available connected lyric services locally. Neither request contacts an external provider.
2. Show selectable identities by human service label. An unavailable saved identity remains visible with a disabled selection and an enabled Remove action. It still counts toward the account's limit.
3. Provide Find lyrics when the user wants another identity or has no saved identity. Let the user choose a connected service and review the search text before pressing Search. Seed the editable query from committed title and artist. Do not send requests on page entry, service selection or each keystroke.
4. Search returns ordinary provider recording candidates. Show title, artist, known album text and duration where supplied. Missing metadata stays unknown. Do not invent album identities, album artwork, creators or availability.
5. Selecting a candidate requests its review metadata. Show that returned metadata and ask the user to confirm that these are the lyrics they want. Accept sends the exact candidate and opaque review stamp returned by backend.
6. On success, refresh local identities and select the returned identity. A distinct Show lyrics action reads it. Reuse the existing explicit reading behavior rather than automatically disclosing searches or accepting a first result.
7. Remove an owned reviewed identity using its exact revision. Do not remove ordinary retained observations through this endpoint. Removing or changing a selection cancels its pending read and clears its text.

At most eight reviewed identities are retained per account and recording, including unavailable ones. Exact duplicate acceptance is idempotent. Explain the limit and offer removal instead of discarding an existing choice automatically. A recording in a shared library can have different private lyric identities for different accounts.

## HTTP contract

All paths below start with `/api/v1/recordings/{id}/lyrics`. Use the existing authenticated API client and its browser origin protections. Responses are private and must not be persisted into a shared/global cache.

| Method and suffix | Operation ID | Request | Response |
| --- | --- | --- | --- |
| GET `/services` | `lyric_identity_services` | none | `RecordingLyricServices`: recording ID and `items` with installation ID, installation revision and human label |
| POST `/candidates` | `lyric_identity_candidates` | `SearchRecordingLyrics`: installation ID, installation revision, query | existing SDK `SearchPage`; no lyric text |
| POST `/identities/review` | `lyric_identity_review` | existing `ProviderCandidate`: installation ID, installation revision, provider recording ID | `RecordingLyricReview`: `candidate` is SDK **RecordingMetadata**, plus opaque `review_stamp` |
| POST `/identities` | `lyric_identity_retain` | `RetainRecordingLyricIdentity`: original `candidate` of type ProviderCandidate plus the exact review stamp | `RecordingLyricIdentity` |
| GET `/identities` | `lyric_identity_identities` | none | `RecordingLyricIdentities`: recording ID and identity items |
| POST `/identities/read` | `lyric_identity_read` | `ReadSelectedRecordingLyrics`: selector and installation revision | `SelectedLyricsLookup` |
| DELETE `/identities/{identity_id}?revision={identity_revision}` | `lyric_identity_remove` | no JSON body | 204 |

Use `candidate.recording` for reviewed recording metadata, not `candidate.title`. `RecordingMetadata.album_title` is plain text and may exist while `candidate.recording.release` is null. The identity response contains `selectable`, `selector`, installation ID/revision, the exact `provider_recording` reference and the service label.

The selector is one of:

```json
{"kind":"observation","observation_id":"<retained observation UUID>"}
```

```json
{"kind":"reviewed","identity_id":"<reviewed identity UUID>","identity_revision":1}
```

The read request sends that exact selector and current installation revision. Do not turn an identity UUID into an observation UUID, derive provider IDs from titles, send audio/source locators or add LRCLIB-specific logic to the client. Removing and re-adding an association produces a different identity UUID; a previous selector must not be reused.

Existing GET `/lyrics/providers` and POST `/lyrics` remain compatible with existing v1 clients. New UI uses the union chooser and selected read so a lyric-only service works for actual local files as well as imported recordings.

## Reading and rendering

`SelectedLyricsLookup.status` is `ready_v1`, `ready_v2`, `unsupported`, `unavailable` or `cancelled`. Both ready variants return the Hito recording ID, exact selector, exact provider recording reference, opaque document digest and typed SDK document. A ready document with `lyrics: null` is a genuine Not found result. It differs from a temporary failure or unsupported extension.

For `ready_v1`, reuse static rendering of existing timed lines. For `ready_v2`, `document.lyrics.content.kind` is `plain` or `anchored`:

- Plain content has `lines: string[]`; retain blank stanza lines.
- Anchored content has `lines: {start_ms, text}[]`; equal anchors and blank clear events are valid. For this slice render its nonempty text as static lines, preserving meaningful stanza/line structure. An anchored blank clear event is not fabricated sung text.
- Render text as text, never HTML. Attribution is optional. It may identify only a provider and canonical HTTPS backlink, with an empty or omitted creator list. Do not call the performing artist the lyric author.

Anchors are on the provider's rendition timeline. These endpoints do **not** prove alignment to the playing audio. No highlighting, karaoke timing, guessed line ends or duration-based alignment belongs in this slice. Existing alignment work must not silently accept a new document or source. Catalog pages must not subscribe to playback ticks.

## Workflow ownership and failures

Use focused hooks for list, search, review/accept, removal and read lifecycles. Shared visual components accept typed values and actions; the page owns its route. Use existing UI patterns and accessibility rather than adding a second design system.

Scope owners to the current account, recording, selected identity and provider revision. Account/recording changes, navigation, changing the provider or candidate, selecting/removing an identity and explicit Cancel retire affected requests. Reject late replies even if a transport abort arrives too late. Preserve unsaved metadata drafts and the active audio/queue.

Validate ready recording ID, selector and provider reference against the initiating request. Treat the document digest as opaque content identity, not proof of alignment. Never keep another account's text or choices on account switching. Do not log query text, titles, lyrics, raw bodies, credentials or URLs; diagnostics use safe correlation, outcomes and timing.

| Result | UI behavior |
| --- | --- |
| no services | Link to existing plugin connection/settings flow; no external request |
| no candidates | Search-specific empty state, editable query and explicit retry |
| ready with null lyrics | Not found, retain the user's chosen identity and offer another search |
| unsupported | Explain that this connected service cannot supply the negotiated lyric format |
| unavailable / upstream error | Temporary failure and explicit retry; never relabel Not found |
| 409 | Context or provider revision changed, busy, or limit reached; use the server error and reload the relevant state before retrying, never blindly reuse the old stamp |
| 401 / 403 / 423 | Retire private work, clear lyric output and use existing authentication/lock handling |
| 404 | Selected recording/identity is no longer accessible; clear stale selection and reload authorized local choices |
| cancelled | End the pending state without a failure toast or late publication |

Keep reading bounded across settings, fetch and body parsing. Backend uses one ten-second workflow deadline. Preserve the existing finite client deadline and do not restart it at each negotiation step. The frontend calls only Hito; Hito handles plugin negotiation, credentials and upstream cooldowns.

## Acceptance for Opus

- Drive the real Lyrics tab through choose service → explicit search → review → accept → local chooser → read, including an authorized local/shared-library recording.
- DOM tests verify exact bodies, plain/anchored/v1 rendering, empty creators, null versus temporary failure, unavailable identities, cap guidance, removal and fresh UUID after re-add.
- Hold replies while switching account/recording/selection, navigating and cancelling. No late text/stamp/selection may appear, no private data remains, and an immediate explicit retry works.
- Assert that merely opening Lyrics or selecting a service performs no candidate search; accepting an identity never calls audio, Sources, Download or metadata-edit endpoints.
- Preserve metadata drafts, playback and queue state. Verify keyboard/focus behavior and escape rendered hostile text.
- Generate API types from actual Rust OpenAPI. Run frontend checks through the tools profile; browser checks use the isolated e2e server/profile, never the persistent normal server volume.
- Final packaged E2E uses the integrated backend and actual LRCLIB service against an owned deterministic upstream fixture. Current protocol mocks are implementation support, not proof of live-provider or platform acceptance.

Frontend may begin now. Backend will provide the actual OpenAPI candidate, the final accepted backend commit/schema, and the composed fixture readiness separately.
