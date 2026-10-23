# Hito

Hito is an open-source audio library built around reliable playback. The monorepo currently runs a Rust server and a shared React browser interface in Docker. The [first public release plan](docs/release-plan.md) includes desktop clients, provider services, downloads, and a community service. Those remain active implementation work; this is a development build.

## Choose your music folders

Install Docker Engine or Docker Desktop with Compose, then open the development setup launcher:

- macOS: open **Hito Setup.command**.
- Windows: run **Hito Setup.ps1** in PowerShell.
- Linux: run `./hito-setup`. The native chooser uses Zenity or KDialog.

Choose folders in the file explorer. Setup builds Hito in Docker and creates read-only mounts automatically. In Hito, create the first administrator account, open **Settings → Library**, and select **Add music folder**. Choose a shared location or one of its subfolders. Each connected folder scans automatically and has its own status, rescan, and removal controls.

Setup retains recognized existing data volumes, project names, ports, and the legacy music mount, and archives a stopped data volume before restarting. It rejects ambiguous installations and unsupported custom mounts. These are development launchers; signed packaging and live native-dialog verification on all three systems remain release work. See [music folder setup](docs/music-folders.md).

## Advanced Docker setup

For a manual instance with synthetic audio:

```sh
docker compose --profile tools run --rm node node /workspace/scripts/audio-fixtures.mjs
docker compose up --build -d server
```

Open [localhost:8080](http://localhost:8080). The generated files are synthetic 30-second WAV tones. The legacy Compose file accepts `HITO_MUSIC_FOLDER` for an advanced single mount. Guided setup does not require that variable. Use the launcher's generated Compose file for subsequent management of a guided installation; see [operations](docs/operations.md).

SQLite persists in a local Docker volume, normally `hito_hito-data`. The server runs as UID 10001 and publishes its port on loopback. Music must be readable by that user. Do not put SQLite on a network filesystem.

The first account administers other accounts from **Settings → Accounts**. New accounts receive no library access until an administrator grants it. Playlists, likes, and history belong to each account.

Browse **Albums** and **Artists** from the sidebar. Albums retain their own covers and disc/track ordering even when a recording appears on several releases. Artist credits remain exact and library-scoped; search folds case and accents without merging identities. See [catalog behavior and limits](docs/catalog-browsing.md).

Open a track's information link to inspect source metadata and save a personal title. **Use source title** restores inheritance. Corrections stay private to the account, and source observations remain available. See [metadata behavior and limits](docs/metadata.md).

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

`./scripts/check-desktop.sh` checks the native audio engine, authorized playback owner, and protected desktop HTTP workflows through a clocked device monitor, then builds the standalone Tauri host and exercises its Linux WebView in Docker. It uses temporary local storage and synthetic audio. The shared UI uses Rust playback in desktop and browser audio on the web. The WebView check verifies actual native output, reload without restarting playback, pause/resume, persisted progress, and joined shutdown. Installers and platform acceptance remain in development. See [desktop ownership](docs/desktop.md), [native playback](docs/native-playback.md), and [engine limits](docs/native-audio.md).

The browser test uses a separate server with temporary storage. It creates its own accounts and never changes the normal server database:

```sh
docker compose --profile tools run --rm node node /workspace/scripts/audio-fixtures.mjs --e2e
docker compose --profile e2e up --build -d e2e-server
docker compose --profile e2e run --rm browser
docker compose --profile e2e stop e2e-server
```

Remove and recreate `e2e-server` before rerunning its first-run test. Browser audio fixtures use a separate directory from development music. Test screenshots and traces stay in ignored `test-results` directories.

## Repository

| Directory                                     | Responsibility                                                                                          |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `apps/server`                                 | Axum JSON API, Inertia adapter, browser security, file streaming, worker lifecycle                      |
| `apps/desktop`                                | Standalone local backend, private WebView access, application storage and exit lifecycle                |
| `apps/community`                              | Separate community ranking service executable                                                           |
| `apps/web`                                    | Shared React UI with browser and native player implementations                                          |
| `crates/core`                                 | Identity, library permissions, SQLite migrations, scan jobs, search, activity, and operation-based sync |
| `crates/audio`                                | Owned-input decoding, optional native device output, transport controls and joined cleanup              |
| `crates/playback`                             | Authenticated queues, authorized native playback, revocation, and joined cleanup                        |
| `crates/plugin-sdk`                           | Apache-2.0 plugin manifest and capability contract                                                      |
| `crates/remote-audio, crates/debrid`          | Remote audio contract and debrid provider clients                                                       |
| `crates/metadata-musicbrainz, crates/testkit` | MusicBrainz metadata adapter and shared test fixtures                                                   |
| `plugins`                                     | Plugin executables: folder, Spotify, YouTube, Jamendo and debrid providers                              |
| `scripts`                                     | Verification, packaging and deterministic audio fixture tools                                           |
| `docs`                                        | Architecture, API contracts, operations, release scope, and verification evidence                       |

See the actual [TODO checklist](TODO.md), [architecture](docs/architecture.md), [operations](docs/operations.md), [API](docs/api.md), and [implementation status](docs/status.md).

## License

Hito uses [AGPL-3.0-only](LICENSE). The independently reusable plugin SDK uses [Apache-2.0](crates/plugin-sdk/LICENSE). No third-party audio or provider credentials are included.
