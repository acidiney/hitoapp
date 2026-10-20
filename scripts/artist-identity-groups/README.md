# Packaged artist identity group browser gate

This opt-in gate exercises a frozen Hito runtime through a real browser and an owned HTTP catalog and playlist provider. It creates an administrator in a fresh tmpfs database, scans two deterministic owned WAV files through the folder picker, approves the provider and imports its playlist through the UI. It never addresses the normal persistent server or publishes a host port.

The account searches both namesakes, previews an exact candidate, confirms each recording's review and separately chooses its artist identity. Two local recordings and one imported recording choose the same exact provider identity. A second imported recording has the same personal credit but chooses the other identity and stays separate. Shared Music and Personal providers remain separate libraries.

The first Choose reaches the real backend. The fixture reads the accepted command and current state, discards only its HTTP acknowledgement and explicitly retries identical request bytes and UUID. Its acknowledgement must equal the original receipt. The page must display the freshly read current choice. No application response is synthesized.

The browser opens chosen cards and recording-detail links, retains legacy credit cards, visits independent library, recording and album pages at limit one, filters a library and reloads. Public group responses must retain original recording, release, reviewed association and library identities. Explicit Reset removes current links and retires empty groups. Before/after snapshots preserve all four recordings' metadata, sources, source choices and progress; both playlists' duplicate entry identifiers; likes, activity, synchronization events, provider preferences and original file hashes; and absent offline downloads. Shared local originals retain canonical 404 source-choice reads, while user-owned imported recordings retain their typed 200 automatic source choices. No Play or Download control is selected.

Existing authorized artwork availability and retrieval remain allowed. The owned provider advertises no artwork, discography, adoption or audio operations, so browsing must not contact it. This does not forbid artwork retrieval from providers that advertise it.

## Qualification status

The frozen application is commit `e0b8c72e804879141fc938e4eeec14204386e179`. The new gate files do not change application source. Existing core, HTTP and DOM behavior has separate red/green evidence; no obsolete image was built to manufacture a packaged red run.

A complete Chromium run passed in 13.539 seconds, including the shell runner and exact cleanup. It used gate input fingerprint `401eb3ad8e0f188b1695457e74be44d3100ba967871cc3128da9a65e0d7818f5`: four accepted choices, one lost-acknowledgement failure, five choose requests, four accepted resets, 13 accepted reads, 33 paired review workflows and 14 canonical absent download-status replies. All forbidden-request, privacy, page-error, malformed-log and bound counters were zero. Subsequent WebKit diagnostics added only safe checkpoints and error categories to the new fixtures, preserving their assertions.

An independent final Chromium repeat passed against all 2,324 inputs at fingerprint `3a6e318b9701f34a8bef905302a728f3b99750aed8112fb8135d4faef7ede15a`: 14.502 seconds for the case and 20.38 seconds for the wrapper, runner exit zero, zero owned survivors and unchanged normal-server identity and health. It completed the same choice, retry, reset, review and download-status outcomes with all forbidden-request, privacy and error counters zero. Final independent security, correctness and performance reviews passed against those same frozen inputs. This README-only evidence update follows that verified snapshot and changes no test or runtime behavior.

WebKit is **not qualified**. Runs reached and passed chosen cards, independent pagination, filters, reload and namesake separation, then reproduced a renderer crash during the final recording document navigation. The browser remained connected; this was not a selector timeout. One instrumented run reached the 1 GiB cgroup memory cap and recorded 2,221 memory max events, with no kernel OOM or OOM-kill event. A controlled 2 GiB experiment also failed, earlier in the review phase, at a 1,316,773,888-byte peak with no max or OOM events. That experiment did not establish memory pressure as the cause. Instrumentation added a small Node wrapper and was not a performance measurement. The maintained harness remains at 1 GiB, with unchanged browser flags and timeouts.

WebKit process/thread admission, shared-memory and temporary-filesystem utilization, and safe graphics/crash classification remain open investigation. This is one functional release gate, not public-release or physical Safari acceptance.

## Immutable runtime inputs

The frozen application image was built from a separate clean product checkout. Its 2,143 product inputs have fingerprint `0ea456d33fbc6d793c52bb63673104ce67f63fbb64266c21e6a90d06d589bfe3`. Gate additions have a separate source manifest; they are not silently included in that product fingerprint.

| Input                                | Existing immutable image                                                  |
| ------------------------------------ | ------------------------------------------------------------------------- |
| Application                          | `sha256:a27a15acab2fbe66435078b9ebd2cbad5c3b1b3ffc07ea37df50934db7813785` |
| Playwright 1.56.1 Noble, Linux arm64 | `sha256:cb5973ffd9cc7a3b0b5389b12904b24e0ed43751169ca47b4ebdc4e8d9fb1be9` |
| Node 22                              | `sha256:cd2df7ff65c5fce1f8a18ce89e3aac24a80c1db3776d5842a4cf86bc27f300a2` |

The shell runner requires these variables and does not build, pull or install dependencies:

- `HITO_PACKAGED_IMAGE`: exact runtime image ID.
- `HITO_PACKAGED_SOURCE`: its exact 40-character source commit.
- `HITO_PACKAGED_INPUTS`: its 64-character product-input fingerprint.
- `HITO_PACKAGED_WORKTREE`: the clean frozen checkout matching those pins.
- `HITO_ARTIST_GROUPS_BROWSER_IMAGE`: existing immutable Playwright image ID.
- `HITO_ARTIST_GROUPS_NODE_IMAGE`: existing immutable Node image ID.

After an explicit execution grant, run `sh scripts/artist-identity-groups/run-packaged.sh chromium`. WebKit requires a separate execution grant while its crash remains open. Each invocation owns a random Compose project, new music and evidence directories, one network-isolated server and one browser sharing only that server's loopback namespace. Existing `hito_node-modules` mounts read-only. The tools profile generates deterministic WAV fixtures into the owned directory. Product and browser inputs mount read-only; the database is tmpfs. No Cargo target or BuildKit output is created by this runner.

The server has a 512 MiB memory limit and 80 MiB of tmpfs. The browser has a 1 GiB memory limit, 256 MiB shared memory, 272 MiB of tmpfs and a 256-process/thread limit. The preparation tool has a 512 MiB memory limit and 64 MiB tmpfs. Browser engines run sequentially. Normal Hito containers and volumes are never dependencies.

## Exact cleanup

Docker commands have deadlines and output bounds. Preparation has a unique container name, run identifier and ownership labels. Cleanup verifies that exact name and all labels before removing its immutable container identifier, including after interruption or timeout. It separately stops and removes the two regular service names in the invocation's random project, checks for remaining project containers and preserves a nonzero incoming or cleanup result. Cleanup commands have their own deadlines. It never removes volumes, images, other projects or orphans.

Two actual network-isolated Node cleanup controls passed: a one-second CLI deadline left an owned preparation container that the helper removed, and a wrong-label case made the helper refuse deletion. The separate control owner then removed its own exact immutable container. Both ended with zero owned resources and unchanged normal-server identity.

## Privacy and request evidence

Screenshots, traces and video are disabled. A bounded console observer checks text and serialized snapshots, correlates review and identity outcomes and checks late private canaries against earlier captures before discarding raw text. Persisted evidence contains counters, capture hashes and private-canary byte lengths and hashes. Account UUID log fields are permitted; credentials, session values, private paths, titles, queries, opaque provider identifiers and signed URLs are forbidden.

The observer accepts only exact same-origin, account-bound download-status GET envelopes for the four owned recording IDs and the actual imported playlist receipt ID. Early witnesses remain bounded and private in memory until those exact targets are registered. Requests must have no query, fragment or body and must carry the expected Hito request/account headers. Replies must be bounded JSON 404 responses with `private, no-store` and the canonical 30-byte absent-resource body. Unmatched, unconsumed or invalid witnesses fail finalization. These reads are distinct from selecting Download. Armed-phase source, adoption, audio, transfer, unexpected mutation and foreign requests remain rejected; authorized artwork-fetch writes remain permitted. Public setup and snapshot calls are separately bounded.

## Adversarial controls

Eleven maintained tests exercise the status observer through an actual owned loopback HTTP service: valid late playlist and recording authorization; foreign recording; wrong account; POST; equally sized noncanonical 404; missing private response header; unregistered or unconsumed witness; query; and wrong Hito request header. They passed without synthesizing application responses.

Four Chromium controls also passed using temporary read-only overlays of the frozen gate, the same real application and a new disposable database for each control. They retained the rejecting assertion verbatim. A control qualified only when its prerequisites, exact rejection latch, expected stage/counters, source pins, zero owned survivors and unchanged normal-server identity all matched. Their Playwright case and shell runner intentionally exited nonzero; setup or engine failures did not count as successful controls.

| Control             | Actual prerequisite and required rejection                                                                                                                                                                                                            |
| ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Retry byte          | Two real identical requests, equal real receipts and completed accepted workflow, then exactly one byte changed in the second captured witness; original equality assertion rejected it.                                                              |
| Retry UUID          | Same real prerequisites, then only the captured request UUID changed to another valid UUID; original equality assertion rejected it.                                                                                                                  |
| Armed write         | Actual authenticated empty sync POST returned its canonical accepted receipt. All positive workflows and exact public-state preservation passed; the original counter assertion rejected exactly one unexpected write.                                |
| Late private canary | An owned canary was emitted and captured before being registered as private. All positive workflows and preservation assertions passed; the original privacy assertion rejected the earlier capture retroactively. Raw captures were never persisted. |

The retry controls corrupt evidence only; they do not test altered commands sent over the wire. Their first byte-control attempt reached the intended assertion but did not qualify: its meta-check expected the accepted UI log before the deliberate failure had allowed the workflow's fresh reads and final log to finish. A bounded observer-completion prerequisite was added before corruption, preserving the strict expected count of one and the original rejecting assertion. The corrected byte and UUID controls passed. An earlier UUID overlay TypeScript narrowing error was a setup failure and is recorded separately.

## Remaining limits

This gate does not establish provider grant revocation, separate installation identity, account switching, every historical replay ordering, active-body cancellation, 100,000-track performance, physical Safari playback or full release readiness. It captures browser console privacy and does not independently inspect runtime server logs. Core and real-router tests cover separate invariants. Normal deployment, WebKit qualification and manual user acceptance remain separate work.
