# Jamendo adapter in development

The `feature/jamendo-plugin` branch contains an independent Rust HTTP adapter and a development executable using Hito's plugin SDK. It is not deployed or ready to offer to Hito accounts. Live credentials, container packaging and host integration remain unfinished.

The operator supplies a private Jamendo client ID. Hito accounts do not supply that credential. The manifest declares search and sources; the adapter returns structured SDK data and adds no plugin UI.

## Ownership

```mermaid
flowchart LR
    HTTP[Thin HTTP routes] --> Catalog[Catalog projection and stable IDs]
    Catalog --> Transport[Bounded provider transport]
    HTTP --> Relay[Explicit audio relay]
    Relay --> Catalog
    Relay --> Framing[Representation validation]
    Framing --> Stream[Bounded streaming body]
    Config[Private operator configuration] --> Transport
```

Configuration fixes the production API endpoint. Provider transport owns independent metadata and media clients with proxies, automatic redirects and retries disabled. Catalog projection owns recording, release and source identifiers. The relay owns stream admission, framing and body lifetime. HTTP handlers validate SDK requests and map typed results.

Manifest, search, detail and source discovery retrieve no audio. An explicit audio request re-reads the selected track and validates its current stream address. The service forwards neither the caller's authorization nor cookies. It exposes opaque IDs instead of upstream media addresses or the client ID.

## Current implementation

| Route | Operation |
| --- | --- |
| `GET /v1/manifest` | SDK capabilities |
| `POST /v1/search` | First bounded search page |
| `POST /v1/recordings/detail` | One recording |
| `POST /v1/sources` | Current playable source facts |
| `GET /v1/audio` | Explicit source relay |

The implementation requests Jamendo's `mp32` format and accepts one documented media host. It does not provide artwork, playlists, downloads or a media cache. The upstream API distinguishes streaming and download addresses; the adapter uses the streaming address. See the [Jamendo tracks API](https://developer.jamendo.com/v3.0/tracks).

Requests have a 4 KiB body limit. Provider JSON has a 256 KiB limit. Metadata and audio each have 16 admission permits. The stream holds its permit through completion or drop, emits at most 64 KiB per chunk and limits each awaited upstream body read to 15 seconds. These implementation bounds do not establish saturation, cancellation or performance acceptance.

## Development executable

The executable accepts a credential file and an optional listen address:

```text
hito-plugin-jamendo --client-id-file /run/secrets/jamendo-client-id --listen 0.0.0.0:8093
```

The default address is `127.0.0.1:8093`. On Unix, the credential must be an absolute path to a regular file owned by the effective user, with one hard link and mode `0400` or `0600`. The final path component cannot be a symbolic link. Reads are bounded, and the client ID accepts at most 128 ASCII letters, digits, hyphens or underscores with an optional trailing newline. Other platforms currently reject credential loading until their file-access policy is implemented. The executable accepts neither an inline credential nor a provider endpoint override.

JSON logs include lifecycle outcomes and correlated request outcomes. The executable permits only its own crate's log targets, even when `RUST_LOG=trace`, and discards invalid filter settings without logging their contents. Initialization failures use static reasons. SIGTERM and SIGINT stop admission and await active HTTP requests before the stopped event.

## Verification and remaining work

Docker checks exercise actual HTTP against owned provider and audio fixtures. The initial workflow validates SDK manifest/search/detail/source responses, exact WAV delivery, zero metadata audio requests, caller-header isolation and omission of private transport data. This fixture verifies transport behavior, not live MP3 compatibility.

A second HTTP case changes the authoritative audio address after source selection. Opening the captured source re-reads provider metadata and rejects the changed origin without contacting it or opening the previous audio. Captured crate logs retain a safe rejection outcome and request correlation while excluding fixture credentials, addresses, search text and musical metadata. Both cases and strict all-target Clippy pass in Docker.

An actual-process Docker test verifies TCP manifest readiness, invalid-request responses, structured correlation and omission of private request and operator inputs under TRACE. It holds an HTTP request at its body read, sends SIGTERM, observes the draining event, completes the request and verifies successful process exit with the listener closed. Both owned HTTP cases, this process case and strict all-target Clippy pass. Independent source reviews accepted the adapter and executable ownership.

Adversarial framing, HEAD/ranges, admission saturation, deadlines, interrupted sockets, rejected credential/startup cases, malformed logging settings, active upstream-audio shutdown and live-provider checks remain release work. The process fixture makes no upstream request, so it does not establish privacy during live provider access.

Live integration needs generic host support for creator/provider credits, a per-recording backlink and persistence restrictions. The existing SDK recording contract cannot express those facts or stop Hito's generic Download action. Jamendo's API terms require attribution and backlinks and restrict offline access; a stream-only adapter alone cannot enforce host behavior. See [clauses 3.3 and 4.1](https://devportal.jamendo.com/api_terms_of_use).

Runtime packaging and those shared contracts must be completed before offering this service to Hito accounts. Fixture acceptance does not replace a configured operator client ID or live checks.
