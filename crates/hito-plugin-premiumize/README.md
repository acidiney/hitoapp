# Premiumize curated HTTP service

This crate provides an Axum router and a `hito-plugin-premiumize` executable for operator-curated recordings bound to existing exact Premiumize cloud files. Provider access is read-only. The service does not discover a library, create transfers or change provider state.

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

Authenticated routes require `Authorization: Bearer …` and `x-hito-plugin-protocol: 1`. Requests and responses use the shared SDK types. Router configuration rejects ambiguous credentials, keys and catalog bindings. The Rust library allows at most 16 connections, bounds each recording's serialized detail to 32,768 bytes and each catalog to 128 entries and 49,152 canonical encoded bytes.

The executable loads exactly one connection from four private files at startup. It does not reload configuration. Its CLI accepts each option and value as separate arguments:

```sh
hito-plugin-premiumize \
  --provider-token-file /run/hito-premiumize/provider-token \
  --service-token-file /run/hito-premiumize/service-token \
  --receipt-key-file /run/hito-premiumize/receipt-key \
  --catalog-file /run/hito-premiumize/catalog.json \
  --listen 127.0.0.1:8099
```

All four file options are required. `--listen` is optional and defaults to `127.0.0.1:8099`; it accepts a numeric IP socket address, including bracketed IPv6. Unknown or repeated options, missing values and relative file paths fail startup. File arguments retain native OS bytes, including non-UTF-8 filenames. There are no inline-credential, environment or dotenv configuration fallbacks.

Private-file loading requires Unix descriptor checks. Every opened input must be a nonempty regular file, owned by the effective process UID, with one hard link and mode exactly `0400` or `0600`. The final path component must not be a symlink. Startup rejects oversized files and changes in file identity, mode, ownership, links, length, modification time or change time during a bounded read. The executable does not write or replace operator inputs.

| Input | Accepted contents |
| --- | --- |
| Provider token | At most 4,098 file bytes. After removing one optional trailing LF or CRLF, 1 to 4,096 ASCII bytes from `!` through `~`. |
| Service token | The same token rules, with a normalized value different from the provider token. |
| Receipt key | Exactly 32 raw bytes, not hex or base64 text; an all-zero key is rejected. |
| Catalog | At most 1,048,576 bytes of JSON, followed by the existing router's stricter entry and canonical-size bounds. |

The catalog object contains `media_origins` and `recordings`. Every recording entry contains `recording`, `source_id`, `label` and `binding`. `recording` uses the SDK [ProviderRecording](../plugin-sdk/src/content.rs); `binding` uses the existing [TransferFileBinding](../debrid/premiumize/src/audio.rs), including its nested [TransferBinding](../debrid/premiumize/src/transfer.rs). Origins must satisfy the shared HTTPS media allowlist. Unknown catalog, entry and SDK recording fields are rejected. The typed binding retains its existing deserialization rules.

Logs are JSON on stdout. `RUST_LOG` controls verbosity within the allowed Hito targets; HTTP/TLS dependency traces remain excluded at `trace`. Startup errors use fixed reasons without operator paths, arguments, credentials or catalog contents. SIGINT and SIGTERM request graceful shutdown; the executable process test qualifies successful SIGTERM after metadata requests with no active audio stream.

Preparation is read-only monitoring. Queued and running transfers report a five-second polling interval; completed or seeding transfers require exact current file verification before Ready. Receipts are opaque, stateless HMAC-SHA256 tags bound to the operation UUID, service authority, full canonical catalog and selected full file binding. Rebuilding clients or routers and reordering configuration preserve receipts when that authority and data stay identical. Catalog, service-bearer or receipt-key changes invalidate them. Replacing the provider account **requires the caller to rotate the receipt key**; the service cannot infer account replacement from the opaque client.

Every audio GET opens a fresh client-owned media stream. Dropping the downstream response drops that stream and its admission lease. The service does not retain signed URLs. Range and If-Range requests are rejected, and HEAD is unsupported. Only whole canonical PCM16 WAV is qualified; general codecs, seeking and failed-seek recovery are not qualified.

Owned qualification passes 12 service tests and all 36 unchanged Premiumize client tests. The service suite includes one executable metadata test and four bootstrap tests covering 46 rejected configurations. It checks native file arguments, six metadata HTTP exchanges, three authorization rejections, nine correlated safe events, joined idle SIGTERM, unsafe input rejection before binding and exact operator-file preservation. Default and `test-support` service Clippy pass with warnings denied. Fresh executable R1 source review found no issues. An independent root replay passed all 48 cases across ten pinned test executables in 99.793 seconds. All 24 binary hash checks and all 2,440 source-file byte/mode pins remained unchanged, with no remaining owned containers.

This qualifies the executable's owned metadata/bootstrap process behavior and the existing router/client protocol fixtures. Container packaging, deployment, Core/application installation and preparation flows, active-audio service shutdown, live accounts, capacity stress, general codecs and seeking remain open. This is not a completed Premiumize application integration or a public release.

Run the checks sequentially from the repository root, following the Docker, free-space and target-isolation rules in `AGENTS.md`:

```sh
docker compose -f crates/hito-plugin-premiumize/tests/compose.runtime.yaml --profile tools \
  run --rm --no-deps -T --pull never rust \
  cargo clippy --offline --locked -p hito-plugin-premiumize --all-targets -- -D warnings

docker compose -f crates/hito-plugin-premiumize/tests/compose.runtime.yaml --profile tools \
  run --rm --no-deps -T --pull never rust \
  cargo clippy --offline --locked -p hito-plugin-premiumize --all-targets \
  --features test-support -- -D warnings

docker compose -f crates/hito-plugin-premiumize/tests/compose.runtime.yaml --profile tools \
  run --rm --no-deps -T --pull never rust \
  cargo test --offline --locked -p hito-plugin-premiumize --features test-support \
  -- --test-threads=1 --nocapture

docker compose -f crates/hito-plugin-premiumize/tests/compose.runtime.yaml --profile tools \
  run --rm --no-deps -T --pull never rust \
  cargo test --offline --locked -p hito-debrid-premiumize --features test-support \
  -- --test-threads=1 --nocapture
```

The fixture Compose file pins an existing Rust tools image, disables networking and pulls, uses cached dependencies offline, sets two build jobs and disables debug information and incremental compilation. Its dedicated target is `/build/premiumize-executable-5dc0b6b5`; use a separate `CARGO_TARGET_DIR` for another checkout or source snapshot. Keep at least 40 GiB of host headroom and combined Hito Cargo targets within 10 GiB. The commands do not enforce those storage bounds automatically. The Linux process fixtures require pidfd support and the tools container's root UID. CHOWN creates the deliberate foreign-owner control; DAC_OVERRIDE allows its intentionally foreign-owned private input to be read and preserved despite its `0400`/`0600` permission rules. They use owned temporary inputs and isolated HTTP/TLS peers, with no live provider credential or normal application data volume. This Compose file is test tooling, not deployment packaging.
