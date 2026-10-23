# Premiumize transfer observation and bound audio

This library observes one caller-configured transfer through
`GET https://www.premiumize.me/api/transfer/list`. The exact transfer ID and
creation timestamp must still match. Missing, duplicated or replaced references
fail closed; a same-name transfer cannot replace the configured reference.

The result preserves that binding, reports queued, running, provider failure,
seeding or completion, and exposes progress as a finite fraction from zero to one.
Provider completion describes a cloud transfer. It does not establish content,
file or recording identity and does not produce a playable source.

The [official API documentation](https://www.premiumize.me/api) recommends the
Bearer authorization header for both API keys and OAuth access tokens. This
client accepts an existing credential through that header only. OAuth enrollment
and credential storage are outside this crate. Transfer observation does not use
names, messages, source URLs, folder IDs or file IDs for matching or include them
in its result. Provider data and credentials are never logged.

The client uses a fixed HTTPS origin, verifies TLS, bypasses proxies, and never
follows redirects or retries. Responses require JSON, identity encoding and at
most 65,536 body bytes. Oversized account transfer lists fail explicitly. Clones
share two admission slots and at least 250 milliseconds between request starts.
The request deadline is ten seconds, with a twelve-second overall observation
deadline including admission pacing. Excess work returns `Busy` without queuing.
Workflow events contain a generated correlation ID, safe outcome, result count
and elapsed time. Cancellation retains the originating log dispatcher.

## Bound cloud-file metadata

`observe_bound_file` freshly validates an exact `TransferFileBinding` against the
transfer list and item details. It returns the unchanged binding, completed or
seeding phase, and current progress. It discards the private link without media
admission or a media request. Availability is metadata, not verified playable
bytes or recording identity. Queued, running and failed transfers remain outside
this ready-only result; `observe` reports their transfer progress separately.

The operation shares the client's two API slots and pacing. One nine-second
deadline covers both requests and validation. Cancellation closes owned I/O,
releases admission and emits one completion under its original log dispatcher.
Metadata observation remains available while two audio bodies retain their
separate media leases.

## Bound cloud-file audio

`open_bound_audio` accepts a separately curated `TransferFileBinding`: the
transfer reference, exact item ID, item creation timestamp, exact provider file
name, byte length and nonempty cloud folder ID. Transfer and item creation times
are independent. A filename does not establish recording identity.

This first capability supports one file in the account's own cloud. A fresh
transfer-list read must still select exactly the configured transfer, retain its
creation timestamp, report finished or seeding, and bind both configured file and
folder IDs. Seeding may have any documented finite progress fraction in `[0, 1]`.
Queued, running, failed, external, folder or missing-file references fail before
an item or media request. Root-folder references with a null folder ID are not
supported.

The file-only `GET /api/item/details?id=...` must retain the exact item ID, name,
byte length, item creation timestamp and folder ID. Its private link is zeroized
after immediate use and is not exposed, serialized or logged. Legacy `type`,
duration and alternate-link fields are not authority. No link lifetime, mint
acknowledgment, retry safety or idempotency promise is inferred from the API.

The caller must supply an explicit trusted media-origin allowlist. The shared
`hito-remote-audio` client validates public DNS addresses, pinned peers and TLS,
and sends no provider credential or cookie to media. Redirects, inconsistent
framing and unsupported bytes fail closed. Only canonical PCM16 WAV is supported;
MIME type alone neither accepts nor rejects audio. Metadata and media opening
share one nine-second aggregate deadline. Two media admission slots are reserved
before API reads and retained by returned bodies until validated EOF, error or
drop. Streaming uses bounded chunks and the shared fifteen-second body idle
deadline. A failed or cancelled opening is not retried automatically.

## Verification

The first executed tracer failed against the unchanged scaffold with
`UnsupportedState`, then passed after implementation. A second behavioral RED
showed that contradictory terminal and queued progress was accepted; the same
test passed after validation was added. Other protocol controls exercise actual
owned HTTPS requests and responses rather than mocking client internals.

The audio scaffold first failed with `UnsupportedAudio`; its unchanged public
TLS tracer then streamed and decoded all 192,000 deterministic PCM samples.
Additional controls cover authority changes, error and response bounds, seeding,
media bytes and framing, actual aggregate deadlines, retained-body admission,
peer-observed cancellation and explicit recovery. They use separate owned TLS API
and media peers. The original transfer-observation tests remain unchanged.

Nine file-observation cases and the original fourteen audio and thirteen transfer
cases pass together. Default and test-support all-target Clippy, formatting,
independent security, correctness and performance reviews pass. An independent
sequential repeat passes all 36 cases, with source and executable hashes unchanged.
Controls verify exact authority, API errors and bounds, cancellation peer closure,
aggregate deadlines, media isolation, complete WAV bodies and every PCM sample.

Run the isolated suite with the existing pinned tools image:

```sh
docker compose -p hito-premiumize-audio \
  -f crates/debrid/premiumize/tests/audio.tools.yaml --profile tools \
  run --rm --no-deps -T --pull never rust \
  cargo test --offline --locked -p hito-debrid-premiumize --features test-support
```

The Compose definition disables external networking, uses an independent Cargo
target and disables debug information and incremental compilation. Its
`test-support` feature allows an owned loopback TLS endpoint with a supplied
certificate authority while preserving production Host and SNI. This test
constructor is absent from default builds.

The owned TLS fixtures do not qualify a live provider account or CDN. Transfer
submission, OAuth enrollment, recording matching, application preparation,
plugin/service integration and a public release remain outside this slice.
