# TorBox curated HTTP service

This library serves an operator-curated collection of exact TorBox torrent-file bindings through Hito's HTTP plugin protocol. It has no executable, container, application installation or live-account acceptance yet. Only the client's canonical PCM16 WAV profile is qualified.

The service owns authentication, structured metadata and a private SQLite preparation journal. The TorBox client owns provider credentials, file verification, generated links and shared audio transport. Hito owns accounts, library records and rendering. Plugins supply no UI or core database access.

`Service::open` requires an explicit state directory, separate private 32-byte key file and `CreateNew` or `OpenExisting` mode. A single cooperative owner holds the private directory claim until `Service::close` joins its bounded journal worker. Stop and join the HTTP listener before closing the service. Reopening authenticates the journal and restores receipts against their exact provider authority and file bindings.

```mermaid
flowchart LR
    Checking[Commit checking receipt] --> Verify[Verify selected file]
    Verify --> Intent[Commit mint intent]
    Intent --> Mint[Request generated link once]
    Mint --> Ready[Commit encrypted ready candidate]
    Mint --> Unknown[Retain uncertain result]
    Ready --> Reopen[Join owner and reopen private state]
    Unknown --> Reopen
    Reopen --> Replay[Replay original operation and handle]
```

An uncertain dispatched mint is never retried automatically. Repeated preparation and status reads retain the original receipt. Catalog changes retire incompatible bindings without erasing their operation claims. Every explicit audio GET freshly verifies the selected file and current operation. Ready candidates survive completed playback, media/control admission rejection and temporary provider failures. Definitive changed bindings retire them. HEAD opens no media; signed addresses and credentials stay private.

The catalog admits at most 128 recordings and 49,152 encoded metadata bytes. HTTP bodies and queries are limited to 4,096 bytes, headers to 32 fields and 8,192 bytes, concurrent work to 16 and response opening to 14 seconds. The existing client's two media permits remain owned through body completion.

The journal retains at most 1,024 lifetime receipts and reserves bounded candidate storage before minting. Database, WAL and SHM limits are 16 MiB, 17 MiB and 64 KiB, with a combined 40 MiB logical and allocated-file ceiling. Capacity rejection precedes provider work and preserves existing ready or uncertain receipts. There is no receipt eviction policy yet. Authentication covers stored rows and their complete receipt set; startup rejects selective receipt deletion. Concurrent external database writers are unsupported. These checks do not establish freshness after restoring an entire valid backup or general protection against live external tampering.

Twenty-one directed service cases pass, including joined owner replacement, lost acknowledgements, catalog changes, private state ownership, authenticated receipt deletion, escaped maximum-length filenames and physical SQLite exhaustion. Both physical-capacity cases preserve their existing receipt across joined replacement; the uncertain operation is not reminted. Scoped strict checks and independent source reviews pass within the single-owner contract. The earlier 32-case client/service checkpoint separately qualifies original-byte WAV delivery, native decoding through EOF and admission recovery. This is fixture evidence, not executable restart, power-loss durability, live-provider compatibility or a playback benchmark.

An independent combined-source Docker run also passes the three restart/filename cases below, with all 170 checked source inputs unchanged and no test containers left behind.

Run a focused restart check through Docker Compose, with the repository's compiler and storage guards:

```sh
docker compose --profile tools run --rm --no-deps -T \
  -e CARGO_TARGET_DIR=/build/torbox-service-check rust \
  cargo test --locked --offline -j2 -p hito-plugin-torbox --features test-support \
  --test restart_durability --test restart_uncertain --test binding_filename \
  -- --test-threads=1
```

Use a separate target directory for each checkout. Other directed cases are in the package's `tests/` directory. The physical-capacity cases each take several minutes.

The service does not submit torrents or magnets, discover a provider library, infer recording identity from provider completion, or promise arbitrary codecs or seeking. Executable private configuration, process/container restart, active-request shutdown, long-term receipt retention, application provisioning, broader media support and live TorBox acceptance remain release work. See [adapter evidence and limits](../../docs/debrid-adapters.md#torbox-curated-wav-service-library).
