# TorBox observation foundation

This crate observes one already configured TorBox torrent file through the provider's HTTPS API. It is a source implementation with deterministic protocol tests. It is not yet connected to a Hito plugin service or application playback.

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
  -f compose.yaml -f crates/hito-debrid-torbox/tests/compose.tools.yaml \
  --profile tools run --rm --no-deps -T --pull never rust \
  cargo test --offline --locked -p hito-debrid-torbox --features test-support
```

## Protocol sources and unfinished scope

The [official OpenAPI document](https://api.torbox.app/openapi.json) identifies the production HTTPS origin, versioned route, bearer authorization and `id`/`bypass_cache` parameters. The [official torrent service documentation](https://github.com/TorBox-App/torbox-sdk-js/blob/main/documentation/services/TorrentsService.md#gettorrentlist) documents `id` returning one object, fresh-information requests and the warning against completion based on `download_state`. Its [response model](https://github.com/TorBox-App/torbox-sdk-js/blob/main/documentation/models/GetTorrentListOkResponse.md) exposes separate completion/presence flags and file identity fields. The [provider's rate-limit policy](https://support.torbox.app/en/articles/13726368-api-rate-limits) documents the per-key endpoint ceiling. These primary sources were checked on 2026-10-20 without a provider account or private API requests.

The inspected primary documentation does not define torrent progress scale, canonical path semantics for file names, file hash algorithms, all ID bounds or every error-state wire string. This observer does not invent those contracts. Submission/control, generated download links, media validation/relay, persistent preparation jobs, plugin deployment and application delivery remain unfinished. Live-provider acceptance still requires an authorized test account and owned content.
