# Premiumize transfer observation

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
and credential storage are outside this crate. Names, messages, source URLs,
folder IDs and file IDs returned by the provider are not used for matching or
included in results and logs.

The client uses a fixed HTTPS origin, verifies TLS, bypasses proxies, and never
follows redirects or retries. Responses require JSON, identity encoding and at
most 65,536 body bytes. Oversized account transfer lists fail explicitly. Clones
share two admission slots and at least 250 milliseconds between request starts.
The request deadline is ten seconds, with a twelve-second overall observation
deadline including admission pacing. Excess work returns `Busy` without queuing.
Workflow events contain a generated correlation ID, safe outcome, result count
and elapsed time. Cancellation retains the originating log dispatcher.

## Verification

The first executed tracer failed against the unchanged scaffold with
`UnsupportedState`, then passed after implementation. A second behavioral RED
showed that contradictory terminal and queued progress was accepted; the same
test passed after validation was added. Other protocol controls exercise actual
owned HTTPS requests and responses rather than mocking client internals.

Run the isolated protocol suite with the existing pinned tools image:

```sh
docker compose -p hito-premiumize-protocol \
  -f crates/hito-debrid-premiumize/tests/compose.tools.yaml --profile tools \
  run --rm --no-deps -T --pull never rust \
  cargo test --offline --locked -p hito-debrid-premiumize --features test-support
```

The Compose definition disables external networking, uses an independent Cargo
target and disables debug information and incremental compilation. Its
`test-support` feature allows an owned loopback TLS endpoint with a supplied
certificate authority while preserving production Host and SNI. This test
constructor is absent from default builds.

No live provider account, transfer submission, media delivery, plugin service,
application preparation workflow or public release is qualified by this slice.
