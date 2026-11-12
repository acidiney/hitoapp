# Independent lyric providers: frontend integration

This specification is ready for parallel frontend implementation by Opus. Backend owns provider requests, authority, identity review, persistence and protocol negotiation. Frontend owns the existing Track Details Lyrics experience and its generic chooser. Plugins supply data, never components or executable UI.

## Candidate plugin credential feedback

This extension belongs to the isolated backend credential candidate. It is not integrated or deployed. The current backend candidate's Rust definitions add `ErrorCode::PluginCredentialsRequired`, optional `ErrorResponse.plugin_connection` with a `PluginInstallationStamp`, and `PluginHealthState::CredentialsRequired`. Its actual Rust-generated contract has SHA-256 `4fb3ad734de58f2496304eca2e9199639427b1b237fc906e9cd75e17fe252355`. Independent comparison preserves all 181 paths and earlier fields while adding only the specified schemas and values; the unchanged schema regression passes. Opus must generate and qualify TypeScript from that candidate artifact before frontend integration, rather than hand-writing API types from this section. The integrated contract elsewhere in this branch remains the earlier version until backend integration.

A rejected plugin service credential returns HTTP 409 with code `plugin_credentials_required` and a safe error message. When current private authority permits it, `plugin_connection` identifies the installation and revision that failed. The response is private and no-store. HTTP 401 still means listener authentication; inaccessible recording or foreign connection requests remain 404 without that stamp. Other 409 responses keep their existing behavior.

Opus owns the frontend implementation:

1. Recognize the exact credential error code through the shared typed error handling. Explain that this music service connection needs its credentials updated, and offer the existing plugin-settings destination. Do not send the listener through sign-in for this error.
2. In plugin settings, render `credentials_required` for the affected account connection with its human service label and existing credential-edit action. A missing health value is unknown, not proof of success. Never display or log the submitted bearer.
3. Apply a stamped error only to the same active account and installation revision. Discard late errors after account switching. If the connection has changed or no stamp is supplied, offer generic plugin settings without marking another connection as broken.
4. Preserve manual lyric selection, library metadata, queue and playback. A failed lyric request does not pause playable audio or remove an identity. Source resolution may continue through another healthy provider; it does not imply every connection failed.
5. After a successful credential update, invalidate the relevant settings and explicit lyric workflow state. Retry only the active, still-authorized request under its normal lifecycle. Public manifest availability alone does not prove protected requests work. Avoid automatic retry loops or subscriptions to playback-position ticks.

Acceptance needs typed handler coverage for this 409 response versus listener 401, inaccessible 404 and unrelated 409 responses; affected versus peer connections; absent or stale stamps; account switching during a held request; successful update/retry; and uninterrupted audio/manual selection. Use an isolated browser server and owned protocol fixtures, never first-run tests against the persistent normal server. Source-search ordering and selected-document recovery have directed backend evidence; metadata/legacy recovery, hydration diagnostics, stale-target dispatch, health deadlines and broader qualification remain open.

## Current evidence and integration status

The manual identity backend is integrated in the development branch in commits `6a75144b` and `1d40e6c8`. The current combined Rust export in [docs/openapi.json](openapi.json) has SHA-256 `ac0cc1e3290410b59ba5865be6ed70436ad218e8ba362273abaefd4ad6229e2a`. Opus owns TypeScript generation from this actual export; do not create a second API model from prose. Automatic Core and HTTP source is integrated in `4f16bf08` and `25e42acb`; the combined backend passes independent verification. This does not qualify the new UI workflow or deploy it.

The review field embeds the SDK metadata shape; use the generated `RecordingLyricReview` candidate type rather than the unrelated host `RecordingMetadata`. The original provisional export `9533036a…` has the wrong reference and must not be used.

At the earlier manual identity checkpoint, a fresh combined-source Docker run passed all 19 new host cases and 72 affected regressions, strict Core/server and production Core checks, formatting and exact API reproduction. Two pre-existing lyric diagnostics remain ignored. This includes Hito → actual LRCLIB router → owned upstream, session/grant/PIN and account isolation, cancellation, one total deadline, dedicated admission and missing-recording retention. All 4,112 source inputs remain exact. The separate LRCLIB service also passes 20 service and 145 SDK cases and one bounded live compatibility check.

The manual Rust definitions are [Core wire types](../crates/core/src/catalog/timed_content/identity_types.rs), [registered HTTP routes](../apps/server/src/api/lyric_identities.rs) and [SDK v2 types](../crates/plugin-sdk/src/lyrics_v2.rs). All seven manual operation IDs remain stable. That checkpoint added six path objects and fifteen schemas without changing earlier paths or schemas. The automatic extension adds its own operation and types described below.

The local LRCLIB development service is running at `http://plugin-lrclib:8101/` for the existing Hito Docker network. No account is installed automatically. Backend integration in source does not update the running Hito server. Finish the UI and isolated full-flow checks before integrating into `main` and rebuilding the local deployment. This does not establish packaged-platform or public-release acceptance.

## Explicit manual chooser

1. Opening the manual chooser lists saved identities locally. It may also list available connected lyric services locally. Neither request contacts an external provider.
2. Show selectable identities by human service label. An unavailable saved identity remains visible with a disabled selection and an enabled Remove action. It still counts toward the account's limit.
3. Provide Find lyrics when the user wants another identity or has no saved identity. Let the user choose a connected service and review the search text before pressing Search. Seed the editable query from committed title and artist. Do not send manual candidate-search requests on chooser entry, service selection or each keystroke.
4. Search returns ordinary provider recording candidates. Show title, artist, known album text and duration where supplied. Missing metadata stays unknown. Do not invent album identities, album artwork, creators or availability.
5. Selecting a candidate requests its review metadata. Show that returned metadata and ask the user to confirm that these are the lyrics they want. Accept sends the exact candidate and opaque review stamp returned by backend.
6. On success, refresh local identities and select the returned identity. A distinct Show lyrics action reads it. Keep this manual reading flow explicit. The automatic workflow below has a separate request and never accepts an identity.
7. Remove an owned reviewed identity using its exact revision. Do not remove ordinary retained observations through this endpoint. Removing or changing a selection cancels its pending read and clears its text.

At most eight reviewed identities are retained per account and recording, including unavailable ones. Exact duplicate acceptance is idempotent. Explain the limit and offer removal instead of discarding an existing choice automatically. A recording in a shared library can have different private lyric identities for different accounts.

## HTTP contract

All paths below start with `/api/v1/recordings/{id}/lyrics`. Use the existing authenticated API client and its browser origin protections. Responses are private and must not be persisted into a shared/global cache.

| Method and suffix | Operation ID | Request | Response |
| --- | --- | --- | --- |
| GET `/services` | `lyric_identity_services` | none | `RecordingLyricServices`: recording ID and `items` with installation ID, installation revision and human label |
| POST `/automatic` | `automatic_recording_lyrics` | strict empty object `{}` of type `ReadAutomaticRecordingLyrics` | `AutomaticLyricsLookup`, ephemeral and private |
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

## Automatic ephemeral read

Automatic loading uses `POST /api/v1/recordings/{id}/lyrics/automatic` once per track/session if no explicit saved lyric choice takes precedence. Opening the explicit manual chooser or selecting a service still sends no candidate search. The POST is the explicit lookup request. GET reads do not trigger it. A saved manual selection always dominates; beginning a manual choice retires pending automatic work.

Send exactly `{}`. Extra fields are rejected. Do not send client title, artist, query, provider or identity. This POST authorizes Core to disclose committed effective title and artist to the first currently approved connected Search+Metadata+Lyrics service, in the same deterministic order as GET `/services`. Core also derives duration and version evidence, requires strict musical matches and rechecks authority, metadata and installation revision. Core picks the first validated timed result in provider order, keeping the first plain result only as a fallback after examining the complete bounded page. It does not try another service after failure or guess through an unexamined continuation.

`AutomaticLyricsLookup.status` has these exact variants:

| Status | Meaning |
| --- | --- |
| `ready_v1` / `ready_v2` | A validated SDK v1/v2 document containing lyrics |
| `no_services` | No active approved connection supports Search+Metadata+Lyrics |
| `insufficient_metadata` | Committed metadata cannot support the bounded confident search; no search was sent |
| `not_found` | No confident match or an actual null lyric result |
| `ambiguous` | An unexamined continuation; multiple compatible identities alone do not trigger this result |
| `unsupported` | No supported negotiated lyric contract |
| `unavailable` | Service failure, invalid data or the total ten-second deadline expired |
| `cancelled` | Explicit Core cancellation |

Ready variants contain exactly the typed fields `recording_id`, `installation_id`, `installation_revision`, `label`, `provider_recording`, `document_digest` and `document`, plus `status`. The service label comes from the committed approved manifest. These results have no selector, observation UUID, retained identity UUID or review stamp. Validate the live account/recording owner, current service installation ID/revision stamp, and exact equality of `document.recording` with `provider_recording` before publication. Bind the opaque document digest to that service/reference result; it is content identity, not an alignment proof. Reject late results after account, track, service revision or manual-selection changes.

Automatic results remain ephemeral. Never call review or retain to implement this path, manufacture a saved selector, or change metadata, Sources, audio, collection state or Download intent. Ordinary 401/403/404/409/423 handling remains in effect; a concurrent lyric workflow can return 409. Aborting the request drops its workflow and provider body. Keep unavailable, ambiguous and not found distinct, with bounded explicit retry rather than automatic service fan-out.

Core now applies timed-first priority across compatible candidates through the automatic endpoint. Protocol v2 negotiation alone does not provide that selection policy. Current packaged browser acceptance remains open. This selection changes neither rendering nor timing/highlighting/alignment behavior.

## Reading and rendering

`SelectedLyricsLookup.status` is `ready_v1`, `ready_v2`, `unsupported`, `unavailable` or `cancelled`. Both ready variants return the Hito recording ID, exact selector, exact provider recording reference, opaque document digest and typed SDK document. A ready document with `lyrics: null` is a genuine Not found result. It differs from a temporary failure or unsupported extension.

For `ready_v1`, reuse static rendering of existing timed lines. For `ready_v2`, `document.lyrics.content.kind` is `plain` or `anchored`:

- Plain content has `lines: string[]`; retain blank stanza lines.
- Anchored content has `lines: {start_ms, text}[]`; equal anchors and blank clear events are valid. For this slice render its nonempty text as static lines, preserving meaningful stanza/line structure. An anchored blank clear event is not fabricated sung text.
- Render text as text, never HTML. Attribution is optional. It may identify only a provider and canonical HTTPS backlink, with an empty or omitted creator list. Do not call the performing artist the lyric author.

Anchors are on the provider's rendition timeline. These endpoints do **not** prove alignment to the playing audio. No highlighting, karaoke timing, guessed line ends or duration-based alignment belongs in this slice. Existing alignment work must not silently accept a new document or source. Catalog pages must not subscribe to playback ticks.

## Workflow ownership and failures

Use focused hooks for list, search, review/accept, removal, selected read and automatic-read lifecycles. Shared visual components accept typed values and actions; the page owns its route. Use existing UI patterns and accessibility rather than adding a second design system.

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
- Assert that opening the explicit manual chooser or selecting a service performs no candidate search. Automatic loading uses only its strict empty-body endpoint once per track/session and never reviews or retains an identity. Explicit manual choices dominate. Accepting an identity never calls audio, Sources, Download or metadata-edit endpoints.
- Preserve metadata drafts, playback and queue state. Verify keyboard/focus behavior and escape rendered hostile text.
- Generate API types from actual Rust OpenAPI. Run frontend checks through the tools profile; browser checks use the isolated e2e server/profile, never the persistent normal server volume.
- Final packaged E2E uses the integrated backend and actual LRCLIB service against an owned deterministic upstream fixture. Current protocol mocks are implementation support, not proof of live-provider or platform acceptance.

The manual contract and composed deterministic fixture remain available. The integrated automatic host extension passes 21 directed cases within an independent 135-case combined backend repeat, alongside nineteen manual identity, fourteen legacy lyric and twenty-four LRCLIB service cases. Two pre-existing lyric diagnostics remain ignored. Strict Core/server all-target checks, production Core checking and formatting pass; all 4,201 inputs remain unchanged. The actual combined API adds four paths and four schemas without changing the 177 prior paths or 424 prior schemas. See [the combined receipt](status.md#2026-11-11-combined-automatic-lyrics-and-playback-lease-backend). Frontend owns generated TypeScript, UI and browser qualification; final local deployment follows the qualified full flow. Timed-first selection is integrated in development at `9b6dbece` after fresh independent review and a 96-case combined-source repeat. See [the selection contract and limits](plugin-timed-content.md#timed-first-automatic-selection). Current browser workflow, deployment, playback alignment and platform/public-release acceptance remain open.


### Timed-first frontend acceptance

The integrated backend keeps the same request, result types and actual Rust API. Frontend continues to use the automatic endpoint, without adding provider-specific search or ranking. For packaged acceptance, drive automatic loading with a plain result before a compatible timed result and require the timed document to display. Verify the plain fallback when no compatible valid timing exists, and a failure state when a later compatible response is unreadable. An explicit manual plain selection must remain selected even when an automatic timed alternative exists. Keep late-response and cancellation guards; this preference establishes no alignment with the audio timeline. Manual candidate Synced/Plain badges remain separate contract work.
