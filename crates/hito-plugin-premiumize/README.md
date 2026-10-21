# Premiumize curated HTTP router

This library builds an Axum router for operator-curated recordings bound to existing exact Premiumize cloud files. It has no executable, deployment configuration or Core installation flow. It does not discover a provider library, create transfers or change provider state.

Each `ConnectionConfig` owns a curated catalog, a `PremiumizeClient`, validated media origins, a service bearer and a receipt key. The service bearer authenticates the caller and selects its connection before request parsing or provider I/O. The provider credential belongs to the client and is separate from that service bearer. Neither credential is forwarded to media requests.

| Method and path | Behavior |
| --- | --- |
| `GET /v1/manifest` | Public SDK manifest with search, sources and source preparation capabilities; playback-only source persistence. |
| `POST /v1/search` | Searches only the selected connection's configured catalog, without provider I/O. |
| `POST /v1/recordings/detail` | Returns configured SDK recording metadata, without provider I/O. |
| `POST /v1/sources` | Freshly observes the exact bound file's availability without opening media. |
| `POST /v1/sources/prepare` | Observes the configured existing transfer and returns a preparation receipt and current phase. |
| `POST /v1/sources/preparation/status` | Verifies the receipt before provider I/O, then observes current progress and exact file readiness. |
| `GET /v1/audio?source_id=…` | Freshly verifies transfer and item authority, then streams the selected whole canonical PCM16 WAV. |

Authenticated routes require `Authorization: Bearer …` and `x-hito-plugin-protocol: 1`. Requests and responses use the shared SDK types. Startup rejects ambiguous credentials, keys and catalog bindings. It allows at most 16 connections, bounds each recording's serialized detail to 32,768 bytes and each catalog to 128 entries and 49,152 canonical encoded bytes.

Preparation is read-only monitoring. Queued and running transfers report a five-second polling interval; completed or seeding transfers require exact current file verification before Ready. Receipts are opaque, stateless HMAC-SHA256 tags bound to the operation UUID, service authority, full canonical catalog and selected full file binding. Rebuilding clients or routers and reordering configuration preserve receipts when that authority and data stay identical. Catalog, service-bearer or receipt-key changes invalidate them. Replacing the provider account **requires the caller to rotate the receipt key**; the service cannot infer account replacement from the opaque client.

Every audio GET opens a fresh client-owned media stream. Dropping the downstream response drops that stream and its admission lease. The service does not retain signed URLs. Range and If-Range requests are rejected, and HEAD is unsupported. Only whole canonical PCM16 WAV is qualified; general codecs, seeking and failed-seek recovery are not qualified.

Seven service cases and all 36 unchanged Premiumize client cases pass with scoped formatting and strict default/test-support Clippy. Fresh R2 source review found no remaining issues. A root independent repeat passed all 43 cases across eight pinned executables in 99.478 seconds, with unchanged source and binary hashes and no remaining owned containers. Evidence includes connection isolation, startup bounds, receipts preserved after router/client reconstruction, replay rejection before I/O, exact WAV bytes and every PCM sample, actual media-peer closure after HTTP cancellation, fresh playback afterward, and safe correlated workflow logs.

This qualifies the router library and owned protocol fixtures. Executable packaging, deployment, Core/application installation and preparation flows, live accounts, capacity stress, general codec support and seeking remain open. It is not a public release or a throughput benchmark.

Repeat the owned cases with the repository's Docker, free-space and target-isolation rules from `AGENTS.md`:

```sh
docker compose -f crates/hito-plugin-premiumize/tests/compose.tools.yaml --profile tools \
  run --rm --no-deps --pull never rust \
  cargo test --offline --locked -p hito-plugin-premiumize --features test-support \
  --test authority --test cancellation --test contract --test detail_budget --test receipts \
  -- --test-threads=1 --nocapture

docker compose -f crates/hito-plugin-premiumize/tests/compose.tools.yaml --profile tools \
  run --rm --no-deps --pull never rust \
  cargo test --offline --locked -p hito-debrid-premiumize --features test-support \
  --test observation --test audio --test file_observation -- --test-threads=1
```

The fixture Compose file pins an existing Rust tools image, named Cargo caches and `/build/premiumize-service-3bcedac7`. Use a different `CARGO_TARGET_DIR` for another checkout or source snapshot. These commands use isolated owned HTTP/TLS fixtures and require no live provider credential.
