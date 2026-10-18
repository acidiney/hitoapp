# Preparing the next browser queue entry

The shared browser player prepares one upcoming queue occurrence while the current track plays. This is deployed to the normal local development server. Native preparation, unavailable-source preparation and release performance acceptance remain open.

## Ownership and behavior

Preparation uses the same successor calculation as natural advancement, including shuffle, repeat and failed entries. It retains the queue occurrence ID, queue revision and playback generation. Position updates do not copy or scan the queue.

```mermaid
flowchart LR
  Current[Current browser audio plays] --> Successor[Predict one actual successor]
  Successor --> Owner[Independent cancellable preparation]
  Owner --> Sources[Current sources then one refresh and resolution]
  Sources --> Buffer[One paused authorized audio element]
  Buffer --> Boundary[Current track ends]
  Boundary --> Check[Recheck current authorized source order]
  Check -->|Same source remains preferred| Adopt[Transfer the prepared element]
  Check -->|Choice or readiness changed| Fresh[Discard buffer and use current sources]
  Adopt --> Current
  Fresh --> Current
```

The preparation owns its source attempts, cancellation signal and paused media. It has a fifteen-second background acquisition/buffering deadline. It can resolve a ready source through approved providers, but does not yet start the separate unavailable-source preparation workflow. Preparation never issues Download or Keep offline commands.

At handoff, current authorized source order is read again. A historical resolution result does not override a newer manual source choice. An exhausted preparation also checks for a source that became ready while the current track played. This final server read means prebuffering alone does not establish a gapless or zero-wait transition.

Queue replacement, removal, repeat/shuffle changes and account teardown cancel obsolete work. Ownership is limited to current media and one preparation. A repeated recording retains distinct queue entry IDs and can reuse a verified current source without repeating provider matching. Buffer transfer releases the outgoing element. If seeking the adopted element fails, ordinary source recovery retains its attempt identity and requested position.

## Verification

The original normal-policy Chromium test failed against the old player: the first owned WAV was playing, but the next recording had no background search. The final prototype package `sha256:9f8d4083b82033f36006577f1a6f9ef42cef24652668a3d004e1b246e66b2e6f` passes the unchanged workflow. It uses the migration-0045 server image with the prepared-player assets. Chromium has no autoplay-policy override.

The browser plays a twenty-second owned WAV, holds the next recording's provider search while current audio advances, verifies the next buffer before the first ends, then observes natural playback on that prepared element without a second matching operation or persistent download. The run took 21.5 seconds, which includes the fixture's first track and is not a latency benchmark. Console checks pass; 5,059 backend JSON records contain none of the fixed private fixture canaries. The disposable host stops with exit zero, no OOM and no restart.

Public player and provider tests cover changed source authority, newly available audio after exhausted preparation, clear/removal during preparation, failed seek recovery, shuffle, repeat one/all and duplicate occurrences. Provider component tests cover Strict Mode replay, account replacement during acquisition, ignored-abort late acknowledgements, adopted-buffer teardown, stale media events and late progress acknowledgements. They verify at most two owned media elements and no Download requests.

Independent security, correctness and performance reviews pass their second and final round. The first round found stale exhausted-source handoff, failed-seek attempt ownership and Strict Mode media cleanup defects. Each had an actual failing regression before its minimal fix. A test-only TypeScript narrowing error was corrected without changing its assertions. The combined main source at `c3cbc85` passes all 258 frontend tests across 82 files, both TypeScript checks and its production build.

The integrated backend passes 299 distinct core cases with one usual ignored fault case, all 139 distinct server cases, formatting and strict core/server Clippy. A download fixture was corrected to wait for actual idle worker turns after safe tracing reproduced its premature completion; application behavior and the original preservation assertions were unchanged.

The combined release image `sha256:6eacc678c8f0ff5380c8947e670bde52f9fd17009db7b040eb474ee5fc48f366`, built from application source `83ae186`, passes fresh Chromium and WebKit workflows in 22.6 and 24.1 seconds respectively. Both play the first owned WAV while the next search is held, buffer the successor before the first ends and naturally advance without a second resolution or Download. The same package includes the HTTP resolution lifetime changes. The Chromium backend log check examines 3,039 JSON records without finding the fixed private canaries; WebKit acceptance includes browser console checks, not that backend-log count.

WebKit initially failed the final audio audit after successful playback. A lifecycle trace and pinned WebKit source identify two GStreamer delegate notifications before the actual HTTP exchanges. The audit now recognizes only exact expected GET URLs with an unrouted, completed status-zero notification and no headers or failure. Its body must be empty or unavailable with the exact `no_resource_data` error. Each notification must pair with a later, distinct routed 200/206 response containing the complete expected audio bytes. Twenty-three contract cases retain rejection of foreign, unpaired, routed-zero, malformed and actual body-read failures. Fresh independent review passes. This is a test classification change, not an application playback fix or physical Safari evidence.

A stopped whole-volume clone rehearsal and a fresh normal backup precede local publication. The migration-only command remains at 0045 with zero migrations applied. Both stopped audits preserve all 106 prior tables, rows and rowids, sequences, receipts, migration checksums, FTS indices, keys, host identity and non-SQLite files. Separate candidate runtime checkpoints change no database rows or managed descendants. Published health, exact OpenAPI/JS/CSS and anonymous player rejection pass, including host checks through loopback and the LAN address. See [deployment evidence](status.md#running-local-deployment).

The later browser Media Session image `sha256:3c2941e1ea009813503c39cd0fb0ca1bfefea5477189d4202c6ae1be5278221c` retains the same Rust application and is now deployed locally. Fresh next-entry workflows on that exact image pass Chromium in 21.6 seconds and WebKit in 22.6 seconds after integration. These remain workflow timings, not latency benchmarks.

Native playback, unavailable/debrid preparation, physical Safari behavior, broader outages, codec coverage, measured start/recovery targets and deterministic gapless verification remain separate gates. See [the release backlog](../TODO.md).
