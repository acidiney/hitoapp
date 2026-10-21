# TorBox curated HTTP service

This library passes its first owned public HTTP/TLS preparation and WAV playback tracer. It has no executable, container, Hito installation or live-account acceptance yet. The isolated workspace registers this crate; all earlier external lock selections remain unchanged.

The collection contains only operator-curated recordings bound to existing exact TorBox torrent files in the accepted client's canonical PCM16 WAV profile. The service owns its bearer credential, opaque public recording/source IDs, process-local preparation handles and private generated-link candidates. Hito owns account connections, the library and rendering. No core database or plugin UI is involved.

The first tracer covers a public manifest and authenticated search/detail/sources and preparation through the generic SDK. Incomplete metadata reports unknown progress with a five-second polling interval. After that interval, the accepted client freshly verifies both completion flags and the exact selected file before minting once. Replayed operations retain their handle. Duplicate prepare and status replay the stored phase without minting or a fresh provider read. Source readiness and HEAD recheck the exact current binding. HEAD opens no media. GET reserves the client's shared media permit before capturing its candidate and operation ID. It borrows the private candidate, revalidates the file through the existing client and rechecks the current operation before returning headers. The tracer verifies all 65,580 WAV bytes, exact EOF and all 32,768 decoded PCM samples.

Startup bounds the catalog to 128 recordings and 49,152 encoded metadata bytes. HTTP policy bounds bodies to 4,096 bytes, query strings to 4,096 bytes, headers to 32 fields and 8,192 bytes, concurrent request work to 16 and response opening to 14 seconds. The shared client retains its two media permits through body completion. This service does not copy its DNS, TLS, framing or decoding policy.

The process-local ledger retains at most 128 operation receipts. A full ledger rejects new operations. A cancelled preparation or uncertain mint blocks automatic retries and new preparation for that source. These paths have not yet received service-level negative qualification. Restart loses the ledger, including its uncertainty history; durable recovery and safe restart behavior must be resolved before service deployment. Further cancellation, readiness and resource-exhaustion controls are also pending.

The admission regression prepares three distinct ready bindings and holds two real HTTP/TLS audio bodies. Rejecting the third GET leaves its original operation, handle and candidate ready without a provider request, media request or fresh mint. Sources and HEAD remain available. After dropping one held body and witnessing its upstream connection close, the same third candidate plays through exact EOF and PCM verification. The complete case observes eleven provider requests, including exactly three mints, and three media GETs. It checks bounded redacted workflow logs and peer closure for both cancelled bodies. Its actual RED failed on the lost Ready receipt; the corrected case passed in 2.60 seconds. This qualifies shared media admission rejection, not every possible opening failure.

Prepared candidates now survive completed playback and can serve another explicit GET. Busy control admission, provider 429 and 5xx responses retain the exact ready receipt for a later explicit retry. Every opening still verifies the full binding and shared media policy. Definitive binding changes retire the candidate with SourceChanged. Concurrent late failures can refine SourceUnavailable to SourceChanged for the same current operation; they cannot revive a candidate or change a newer operation. No automatic retry, renewal or link lifetime is introduced.

Eight new service cases and the two previous service cases pass with all 22 prior client cases, strict default/test-support checks, formatting and final independent source review. An independent read-only Docker repeat passes the 32 cases across six pinned binaries in 58.607 seconds, with unchanged source and binary hashes and zero remaining containers. This is source qualification, not a deployable service or a playback benchmark.

The accepted TorBox client owns fixed HTTPS provider requests, exact selection parsing, query-token link minting, uncertainty after dispatch, and shared media origin/DNS/TLS/framing/audio ownership. The service reuses those boundaries. A provider-completed file does not prove recording identity or audio compatibility. No link lifetime is invented. A signed media URL remains private and can contain its own credentials; the opener does not copy Authorization, Cookie or Referer headers to it.

The requestdl contract is documented in [the provider OpenAPI](https://api.torbox.app/openapi.json). The service will not submit torrents or magnets, discover a provider library, silently retry uncertain metered link requests, or claim arbitrary codec/seek support. Durable preparation recovery after service restart, executable private configuration, packaging, live-account acceptance and generic Hito browser installation remain later work. Further negative protocol and lifecycle cases follow the first actual RED/GREEN loop.

Repeat the owned tracer through Docker Compose, with the compiler and disk-space checks required by the repository:

```sh
docker compose -p hito-torbox-service-tools -f compose.yaml -f crates/hito-plugin-torbox/tests/compose.tools.yaml \
  --profile tools run --rm --no-deps -T --pull never rust \
  cargo test --offline --locked -p hito-plugin-torbox --features test-support \
  --test contract -- --exact authenticated_preparation_waits_then_revalidates_only_its_bound_wav_through_eof --nocapture
```

The initial dedicated target was `/build/torbox-service-9279340`; the reusable-audio snapshot uses its separate `/build/torbox-reusable-9a7fd9d` target. Choose another isolated target for a different checkout. The first collected RED compiled successfully, then failed on the scaffold's public manifest response, 501 instead of 200. The complete tracer subsequently passed. With retained readiness after EOF it now observes seven exact provider requests and one media request, with zero media requests during HEAD, and checks redacted correlated completion logs. This is functional fixture evidence, not a latency benchmark or full provider release gate. No private accounts or live provider calls are authorized by this fixture.

Run the admission regression with the same Compose prefix and `cargo test --offline --locked -p hito-plugin-torbox --features test-support --test admission`. The original service tracer and all twenty-two existing TorBox client audio/observation cases also pass after the admission change. Strict Clippy passes for the service's default and test-support targets.

The reusable-audio cases use `--test reusable_audio --test control_admission` with the same package and feature. Override `CARGO_TARGET_DIR` through Compose for the exact isolated checkout; do not reuse another source tree's compiler output.
