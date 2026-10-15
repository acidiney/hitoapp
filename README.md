# Hito

Hito is an open-source audio library built around reliable playback. The monorepo currently runs a Rust server and a shared React browser interface in Docker. The [first public release plan](docs/release-plan.md) includes desktop clients, provider services, downloads, and a community service. Those remain active implementation work; this is a development build.

## Run in Docker

Docker Engine and Docker Compose are the only host requirements.

```sh
cp .env.example .env
docker compose --profile tools run --rm node node /workspace/scripts/audio-fixtures.mjs
docker compose up --build -d server
```

Open [localhost:8080](http://localhost:8080). Create the first administrator account, then select **Scan music folder**. The generated files are synthetic 30-second WAV tones for verification.

The current development build uses manual mount configuration. To use your own audio, set `HITO_MUSIC_FOLDER` in `.env` to an absolute folder path and recreate the server:

```sh
docker compose up -d --force-recreate server
```

Music mounts read-only at `/music`. SQLite persists in the `hito_hito-data` Docker volume. The application runs as UID 10001 and publishes its port on loopback. The configured folder must be readable by that user. Do not put SQLite on a network filesystem.

The approved installation flow will offer a native folder picker and configure Docker mounts automatically. Multiple-folder administration will live in Settings. These remain the next implementation work; nontechnical users will not need to edit environment variables in the release flow.

The first account administers other accounts from **Settings → Accounts**. New accounts receive no library access until an administrator grants it. Playlists, likes, and history belong to each account.

The library has artwork cards and a list view, a recording spotlight, personal playlists, and a persistent player with an expanded queue. Covers come from embedded tags or neighboring cover files; files without covers use a neutral placeholder. Settings groups library scans, privacy information, and account administration into separate tabs.

## Develop and verify

Run builds and tests inside Docker:

```sh
docker compose --profile tools run --rm rust cargo test --workspace --locked
docker compose --profile tools run --rm rust cargo fmt --all --check
docker compose --profile tools run --rm rust cargo clippy --workspace --all-targets --locked -- -D warnings
docker compose --profile tools run --rm node npm ci
docker compose --profile tools run --rm node npm test
docker compose --profile tools run --rm node npm run build
```

`./scripts/verify.sh` runs the full development check. See [development](docs/development.md) for the pinned Rust toolchain, editor setup, and generated API contracts.

The browser test uses a separate server with temporary storage. It creates its own accounts and never changes the normal server database:

```sh
docker compose --profile tools run --rm node node /workspace/scripts/audio-fixtures.mjs --e2e
docker compose --profile e2e up --build -d e2e-server
docker compose --profile e2e run --rm browser
docker compose --profile e2e stop e2e-server
```

Remove and recreate `e2e-server` before rerunning its first-run test. Browser audio fixtures use a separate directory from development music. Test screenshots and traces stay in ignored `test-results` directories.

## Repository

| Directory | Responsibility |
| --- | --- |
| `crates/hito-core` | Identity, library permissions, SQLite migrations, scan jobs, search, activity, and operation-based sync |
| `crates/hito-server` | Axum JSON API, Inertia adapter, browser security, file streaming, worker lifecycle |
| `crates/hito-plugin-sdk` | Apache-2.0 plugin manifest and capability contract |
| `apps/web` | Shared React UI and browser playback implementation |
| `scripts` | Deterministic audio fixtures |
| `docs` | Architecture, API contracts, operations, release scope, and verification evidence |

See the actual [TODO checklist](TODO.md), [architecture](docs/architecture.md), [operations](docs/operations.md), [API](docs/api.md), and [implementation status](docs/status.md).

## License

Hito uses [AGPL-3.0-only](LICENSE). The independently reusable plugin SDK uses [Apache-2.0](crates/hito-plugin-sdk/LICENSE). No third-party audio or provider credentials are included.
