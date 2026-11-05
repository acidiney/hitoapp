# Hito

Hito is an open-source audio library built around reliable playback. The monorepo currently runs a Rust server and a shared React browser interface in Docker. The [first public release plan](docs/release-plan.md) includes desktop clients, provider services, downloads, and a community service. Those remain active implementation work; this is a development build.

## Try Hito

### What you need

- Docker running, with Compose: [Docker Desktop](https://www.docker.com/products/docker-desktop/) or [OrbStack](https://orbstack.dev/) on macOS, Docker Desktop on Windows, Docker Engine on Linux.
- Git, to clone this repository.
- About 10 GB of free disk space. The first build takes 5 to 15 minutes, depending on the machine.
- A folder with some music files. Hito only reads it; it never changes or moves your files.
- Port 8080 free on your machine. Hito listens on `http://localhost:8080`, and only on your computer.

### Start it

1. Clone the repository and open its folder.
2. Run the setup launcher for your system:
   - **macOS:** double-click **Hito Setup.command** in Finder. If macOS says it cannot be opened, right-click it and choose **Open**, or run `./"Hito Setup.command"` in Terminal.
   - **Windows:** right-click **Hito Setup.ps1** and choose **Run with PowerShell**.
   - **Linux:** run `./hito-setup`. The folder picker needs Zenity or KDialog.
3. Pick one or more music folders when the folder picker opens.
4. Wait while setup builds Hito in Docker. A Terminal window shows the progress.
5. Your browser opens `http://localhost:8080`. Create the first account. That account is the administrator.
6. In **Library**, select **Add music folder** (it is also under **Settings → Music & providers**). The folder you picked in step 3 is listed; open it and select **Use this folder**. Hito scans it, and your music appears in **Library**. Press play on any track.

To add more folders later, run the launcher again. It keeps your existing folders and library.

### If setup fails

- The launcher prints the reason the step failed. Fix it and run the launcher again. Your music and library data are kept.
- **Docker is not running:** start Docker Desktop or OrbStack, wait until it is ready, and try again.
- **Port 8080 is in use:** stop whatever uses it, then run the launcher again.
- Setup keeps its files in `~/Library/Application Support/Hito` on macOS, `~/.config/hito` on Linux and `%LOCALAPPDATA%\Hito` on Windows. `setup.log` lists the steps, and a failed step leaves its full output in `<step>.out` next to it.

### Optional services

Hito works with only your own files. You can also run optional services, called addons, next to it: Spotify (import public playlists) and YouTube (search and audio) first, and others such as Jamendo. Start Hito with both in one command from the repository folder:

```sh
docker compose -p hito -f compose.yaml -f deploy/compose/addons.yaml --profile plugins up -d --build
```

The server offers both addresses in **Settings → Plugins**. Each account then connects them. The setup launcher can do the same: answer **Start addons** when it asks. The [self-hosting guide](docs/self-hosting.md) lists all nine addons, what each needs, how to connect, update, stop and remove them, and what to do when one fails.

### Stop it

```sh
docker stop hito-server-1
```

`docker start hito-server-1` starts it again. Your library lives in the Docker volume `hito_hito-data`; removing the container does not remove it.

These are development launchers, not signed installers. See [music folder setup](docs/music-folders.md) for how folders are shared and kept read-only.

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

Run builds and tests inside Docker. The root `Makefile` is the single task entry. `make help` lists the targets.

```sh
make test-rust     # cargo test for the whole workspace, plus the ignored storage walk test
make test-web      # vitest
make web-install   # npm ci (root workspace, installs into apps/web/node_modules)
make web-audit     # npm audit, high severity
make lint          # clippy with warnings denied
make web-lint      # prettier, tsc and eslint (React hooks rules)
make fmt           # rustfmt, rewrites files
make fmt-check     # rustfmt check
make contracts     # OpenAPI and TypeScript regeneration check
make e2e           # isolated browser server plus Playwright
make verify        # full development check (scripts/verify.sh)
```

[Development](docs/development.md) covers isolated Compose projects, Cargo targets per worktree, the disk budget and running a single test. [Testing](docs/testing.md) explains the test layers.

`./scripts/check-desktop.sh` (or `make desktop`) checks the native audio engine, authorized playback owner and protected desktop HTTP workflows through a clocked device monitor, then builds the standalone Tauri host and exercises its Linux WebView in Docker. It uses temporary local storage and synthetic audio. The shared UI uses Rust playback in desktop and browser audio on the web. Installers and platform acceptance remain in development. See [desktop ownership](docs/desktop.md), [native playback](docs/native-playback.md) and [engine limits](docs/native-audio.md).

The browser test uses a separate server with temporary storage. It creates its own accounts and never changes the normal server database. Run it with `make e2e`; it recreates `e2e-server` each time, so the first-run test always starts clean. Browser audio fixtures use a separate directory from development music. Test screenshots and traces stay in ignored `test-results` directories.

## Repository

```text
apps/
  server/      Axum JSON API, Inertia adapter, browser security, file streaming, worker lifecycle
  desktop/     Standalone Tauri host with a private local backend and native playback
  community/   Separate community ranking service executable
  web/         Shared React UI (browser and native player implementations)
crates/
  core/        Identity, catalog, collection transactions, library storage, plugins, SQLite
  audio/       Owned-input decoding, native device output, transport controls
  playback/    Authenticated queues and authorized native playback
  plugin-sdk/  Apache-2.0 plugin wire contract and curated catalog admission
  remote-audio/ Remote audio contract shared by the debrid clients
  metadata-musicbrainz/  MusicBrainz metadata adapter
  testkit/     Shared test helpers: log capture, TLS, plugin HTTP server, WAV fixtures
  debrid/      Provider clients: real-debrid, all-debrid, torbox, premiumize
plugins/       Plugin services: folder, spotify, youtube, jamendo, real-debrid, all-debrid, torbox, premiumize
docker/        Dockerfiles for the server, community service and plugin images (one shared plugin recipe)
deploy/        Plugin Compose overlays (deploy/compose) and VPS deployment files
scripts/       Verification, setup, packaging and audio fixture tools
docs/          Architecture, API contracts, operations, release scope and evidence
```

The root also holds `compose.yaml`, `Makefile`, `Hito Setup.command`, `Hito Setup.ps1` and `hito-setup`. Package names keep the `hito-` prefix, for example `hito-core` lives in `crates/core`.

Start with the [documentation index](docs/README.md). Other entry points are the [TODO checklist](TODO.md), [architecture](docs/architecture.md), [operations](docs/operations.md), [API](docs/api.md) and [implementation status](docs/status.md).

## License

Hito uses [AGPL-3.0-only](LICENSE). The independently reusable plugin SDK uses [Apache-2.0](crates/plugin-sdk/LICENSE). No third-party audio or provider credentials are included.
