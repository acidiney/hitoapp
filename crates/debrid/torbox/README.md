# TorBox observation foundation

This crate observes one already configured TorBox torrent file through the provider's HTTPS API. It has deterministic protocol tests and supports the separate curated HTTP service library. That service is not deployable yet, and application playback integration remains open.

## Observation contract

The caller supplies a separate provider token and one trusted torrent/file binding. Observation reads only `GET https://api.torbox.app/v1/api/torrents/mylist?id=ID&bypass_cache=true`. It requires an object response and exact torrent ID, info hash, total bytes, selected file ID, provider file name and selected bytes. File names remain exact provider strings. They are not filesystem paths and receive no normalization or basename substitution. A missing selected file, duplicate selected file ID or any changed bound field fails closed without changing the caller's binding.

Both `download_finished` and `download_present` must be true for `ProviderCompleted`. This is Hito's conservative metadata policy. The `download_state` string is not used as a completion predicate; it does not override either flag. Completion never means playable media, verified recording identity or content integrity. Missing or mistyped required fields, malformed JSON and explicit provider errors fail closed.

Hito currently accepts nonzero torrent IDs and file IDs including zero, lowercase 40-character hexadecimal torrent info hashes, positive sizes, selected size no greater than total size, and nonblank file names of at most 4,096 UTF-8 bytes without controls. The response must fit 65,536 bytes. These are deliberately restricted supported inputs, not assertions about TorBox's full API range. The file list is bounded by that response size; this implementation has no separate 512-entry limit.

Production uses its fixed HTTPS origin with verified certificates, a sensitive bearer header and no proxy discovery, redirects or automatic retries. Test support retains the production Host/SNI and admits only an owned CA with a nonzero loopback socket. One client and its clones share two operation permits and 250 ms request-start spacing. Separate constructors do not coordinate an account-wide limit. Observation has one 12-second deadline, a two-second connection limit and a ten-second HTTP timeout. Dropping its future releases admission and owned request work. Structured debug outcomes include a private correlation ID, count and elapsed time; they exclude credentials, raw provider details and bound identity strings.

## Verified source behavior

Twelve public protocol cases pass through an owned loopback TLS server. The original four-read tracer checks `completed` with false/true flags, true/false flags, both true, then changed selected-file identity. A same-name, same-size decoy retains a different file ID. The tracer verifies exact bodyless authenticated requests, the unchanged persisted binding and correlated private-data-free outcome logs.

The other cases cover malformed configuration before dispatch, constructor restrictions, all bound identity fields, ambiguous selected files, explicit provider errors, HTTP rejections, redirects without follow-up requests, content type/encoding/header limits, declared and streamed body bounds, exact-boundary JSON, malformed provider fields, shared admission/pacing, cancellation and deadline recovery. These fixtures do not establish live TorBox compatibility or release performance targets.

Run from the repository root with the existing pinned Rust tools image and dedicated Cargo target:

```sh
docker compose -p hito-torbox-protocol-checks \
  -f compose.yaml -f crates/debrid/torbox/tests/compose.tools.yaml \
  --profile tools run --rm --no-deps -T --pull never rust \
  cargo test --offline --locked -p hito-debrid-torbox --features test-support
```

## Protocol sources and unfinished scope

The [official OpenAPI document](https://api.torbox.app/openapi.json) identifies the production HTTPS origin, versioned route, bearer authorization and `id`/`bypass_cache` parameters. The [official torrent service documentation](https://github.com/TorBox-App/torbox-sdk-js/blob/main/documentation/services/TorrentsService.md#gettorrentlist) documents `id` returning one object, fresh-information requests and the warning against completion based on `download_state`. Its [response model](https://github.com/TorBox-App/torbox-sdk-js/blob/main/documentation/models/GetTorrentListOkResponse.md) exposes separate completion/presence flags and file identity fields. The [provider's rate-limit policy](https://support.torbox.app/en/articles/13726368-api-rate-limits) documents the per-key endpoint ceiling. These primary sources were checked on 2026-10-20 without a provider account or private API requests.

The inspected primary documentation does not define torrent progress scale, canonical path semantics for file names, file hash algorithms, all ID bounds or every error-state wire string. This observer does not invent those contracts. Submission/control, generated download links, media validation/relay, persistent preparation jobs, plugin deployment and application delivery remain unfinished. Live-provider acceptance still requires an authorized test account and owned content.

## Verified preparation before minting

`TorBoxClient::verify_bound_file` returns a move-only `VerifiedFilePreparation` before sending the generated-link request. It retains the originating client, exact copied binding, actual shared control permit and original 12-second deadline. Consuming `mint()` sends one request. Time spent verifying or waiting in caller code remains part of that budget, and expiry is checked again after request pacing before dispatch. The existing `prepare_bound_file` delegates through these same stages.

This boundary lets a future service commit its durable intent before minting. Dropping the capability or its unpolled mint future sends no mutation and releases admission. Cancellation after dispatch retains the original uncertain outcome and safe completion log. This change adds no durable journal, candidate storage or restart recovery.

Six new public TLS cases and all 32 prior client/service cases pass, with strict affected default/test-support checks and formatting. Independent correctness, security and performance reviews and a separate 38-case repeat pass. The repeat took 85.242 seconds with source and binaries read-only, unchanged hashes and no remaining containers. The cases cover the explicit mint boundary, verification plus caller-delay expiry, drops, shared admission and active-mint timeout/cancellation with actual peer closure. Dedicated expiry while waiting specifically in request pacing remains unqualified.

## Authenticated candidate persistence

`TorBoxClient::seal_candidate` returns versioned authenticated ciphertext bounded by `SealedCandidate::MAX_BYTES` (16,384 bytes). `CandidateContext` binds it to the state, operation and handle UUIDs, stable recording/source IDs and complete file binding. Authentication also covers the fixed provider origin and a keyed provider-credential authority. Private URLs remain inaccessible through the public API.

Only the originating client or its clones can seal a minted candidate. `restore_candidate` accepts a fresh client with the same credential, key and authenticated context. Both operations are local and perform no provider, media or filesystem I/O. Opening a restored candidate still freshly verifies its bound file before media access. The caller owns private key persistence and ciphertext storage; the service has no durable journal or restart recovery yet.

Three public TLS workflows pass: restoration across destroyed client owners followed by two exact WAV/PCM opens with one total mint; foreign-client, wrong-key, changed-credential and tamper rejection; and all context fields, malformed envelopes and safe correlated completion logs. All 38 legacy cases also pass, including a separate 41-case repeat. These fixtures do not establish live-provider compatibility.

## Generated links and bounded audio

This crate prepares one opaque generated-link candidate after freshly observing its exact completed file binding. The original opening API consumes that candidate; `BoundAudioOpening::open_retained` can instead borrow one retained by its service owner. Both observe the binding again before the shared `hito-remote-audio` opener validates an explicitly approved media origin, public DNS pinning, TLS, response framing and canonical PCM16 WAV. Candidates own zeroizing private URLs without URL getters, Debug or serialization. The result does not prove recording identity or content integrity.

The [official generated-link route](https://api.torbox.app/openapi.json) requires the API token in the `token` query parameter, rather than a bearer header. Hito sends the caller's exact torrent/file IDs, including file ID zero, and explicit `zip_link=false`, `redirect=false`, `append_name=false`. It omits `user_ip`. A valid acknowledgment supplies one JSON string candidate. The [official SDK contract](https://github.com/TorBox-App/torbox-sdk-js/blob/main/documentation/services/TorrentsService.md#requestdownloadlink) describes a metered expiring link and contains inconsistent start-window wording, so Hito assigns no durable lifetime. A dispatched minting GET without a usable acknowledgment is uncertain and receives no automatic retry. Explicit authentication denials and well-formed provider rejections retain typed errors. A cancelled dispatched mint logs uncertainty.

The shared opener copies no provider Authorization, Cookie or Referer headers into media requests. The returned signed URL has its own private query; arbitrary provider-returned URL content is not claimed credential-free. Two media permits remain owned through body EOF or drop. Opening has a nine-second deadline; body reads retain the shared fifteen-second idle deadline and exact length/EOF checks. These are implementation bounds, not release latency measurements.

Ten audio protocol cases and the original twelve observation cases pass in isolated Docker Compose tools. The first audio tracer failed against the unsupported scaffold, then passed unchanged after implementation. Remaining audio controls passed on first observation, except one incorrect test oracle that initially expected misleading MIME to override valid WAV bytes. The shared byte-validation policy remained unchanged: the positive now consumes exact bytes through EOF and decodes every PCM sample; actual HTML/non-WAV, malformed WAV, framing, redirect and origin negatives still fail.

The audio cases cover exact signed-query requests, fresh incomplete/changed-file denial before media, file ID zero, lost/malformed/oversized/cancelled/timed-out acknowledgments, no automatic retry, private correlated logs, shared active-body admission, body drop and cancellation during media qualification. Peer-closure witnesses complete before forced fixture shutdown while another client remains alive. Default and test-support configurations pass strict all-target Clippy and formatting. The separate service's reusable-audio qualification is recorded in [its README](../../hito-plugin-torbox/README.md). Live-provider acceptance and application playback remain open.

```sh
docker compose -p hito-torbox-audio-checks \
  -f compose.yaml -f crates/debrid/torbox/tests/audio.compose.tools.yaml \
  --profile tools run --rm --no-deps -T --pull never rust \
  cargo test --offline --locked -p hito-debrid-torbox --features test-support
```
