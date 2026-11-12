# Working on Hito

Hito is a self-hosted music player with an open plugin system. A Rust core owns the library. A React interface runs in the browser and inside a Tauri desktop app. Plugins are separate HTTP services that add search, playback sources, playlists, track details, covers and lyrics.

Maintainers keep local, untracked working notes in `.agents/meta/`. If `.agents/meta/README.md` exists, read it before starting; it describes the current plan and where to record progress. Never describe a development build as a public release.

## What we never compromise on

### 1. Playback that tells the truth

Hito exists to play music reliably. A track marked Ready plays. When a source fails, Hito tries the next one in the listener's order and says what happened. A spinner that never ends, a Ready label on a dead source or a silent failure is a bug.

### 2. Plugins stay outside and stay optional

Hito never downloads, loads or runs plugin code. It talks to plugins over HTTP through the SDK contract, and it works the same with a reference plugin, someone else's plugin or none at all. No feature may assume that a particular plugin is installed. A listener approves a plugin's capabilities before connecting it.

### 3. Every account is private

Playlists, likes, history, corrections, credentials and listening data belong to one account. Administrators grant library access; they do not see other accounts' private state. Contributions to shared services are separate opt-ins, off by default.

### 4. The listener's files are read-only

Music folders are mounted read-only. Edits in Hito never write back to a file. Downloads and saved copies happen only after an explicit action.

### 5. Every surface

Hito has three surfaces, and a feature is not done until each one has been considered:

- **Server and browser.** `hito-server` serves the API and the web app. The web app also installs as a phone or desktop web app.
- **Desktop.** The Tauri host runs the same core in-process with its own private library, and plays audio through the native Rust player instead of browser audio.
- **Plugin services.** Each reference plugin under `plugins/` is its own program, built against `crates/plugin-sdk`.

People should be able to run the server in Docker or directly on the host, and the desktop app should install without Docker. Development and tests run in Docker today.

## How we want changes made

Prefer the smallest model that makes the correct behavior unsurprising. Do not keep complexity just because it exists, and do not add machinery because it looks architectural. Understand the real constraint first. Measure twice, cut once, and build only what the task needs. The developer's instructions override anything here.

Several agents may work in this repository at the same time, each in its own worktree. Assume the processes, containers and volumes you did not start belong to someone else.

## Glossary

- **Account**: one person signed in to a server. The first account is the **administrator**.
- **Library**: a set of music an administrator grants to accounts. A **music folder** is a read-only folder the server scans into a library.
- **Recording**: one piece of music with a stable ID. Its identity survives tag edits and moved files.
- **Source**: one way to get audio for a recording, such as a local file, a plugin result or a saved copy.
- **Plugin**: an HTTP service that declares **capabilities** in its manifest. An administrator **offers** a plugin to the server. An account **connects** it, which creates an **installation**.
- **Core**: `hito-core`, which owns identity, the catalog, collection transactions, indexing, plugins and SQLite.
- **Host identity** is the backend's identity. **Sync client identity** is a synchronizing client's identity. They are different concepts.

## The ways to hurt yourself

1. **Touching the live install.** The developer's own server is the `hito` Compose project. It keeps its data in the `hito_hito-data` volume and listens on port 8080. Never run first-run or browser tests against it, never open its database read-write, and never run `make up` or `make down` unless asked, because both act on that project. Use the `e2e` profile's isolated server for browser tests.
2. **Removing by pattern.** Never stop, kill or remove a process, container or volume you found by matching a name. Other agents' `hito-*` containers and the developer's servers and plugins run beside yours. Remove only what you started, by the ID you captured.
3. **Sharing build output.** Give every worktree its own Compose project and Cargo target with `make COMPOSE_PROJECT=<name> ...`. Two source trees over one target reuse stale timestamp-based artifacts.
4. **Filling the disk.** Keep at least 40 GiB free on the host and all Hito Cargo targets within 10 GiB. Check before and during large builds, and run compiler-heavy checks one at a time. At either limit, stop starting builds and remove only inactive, regenerable Hito output. Never run a global Docker prune, and never remove another project's artifacts or data.
5. **Committing working notes.** Plans, evidence logs, reviews and handoffs live in `.agents/meta/`. Never stage or force-add them.

## Hit every surface

Before calling work done, walk this list and say which entries applied:

- **Entry points.** A behavior reachable from one page is usually also reachable from Search (Ctrl K), the queue, a row menu and the phone tab bar.
- **Clients.** Browser audio and the native desktop player both implement the shared `Player` interface. A playback change needs a decision for both.
- **Plugins.** Plan for a plugin that is missing, unavailable, out of date, asking for credentials or lacking the capability. Each capability needs its own decision.
- **Accounts.** Check administrator and member, shared libraries, a locked screen and a deleted account.
- **Contracts.** HTTP changes go through Rust wire types. Regenerate `docs/openapi.json` and the TypeScript client, and commit both.
- **Reverse states.** A way in needs a way out and a way to see it. Hide needs restore, follow needs unfollow, and download needs remove.
- **Failure states.** Offline server, slow provider, cancelled request and app restart mid-task.
- **Docs.** Check whether the change makes a page in `docs/` wrong.

## Dev environment

Builds and tests run through Docker Compose. Host tools may edit and inspect files. The `Makefile` is the entry point, and `make help` lists the targets.

- `make check-core`, `check-server`, `check-playback`, `check-plugins`, `check-community` and `check-web` run one area.
- `make lint`, `make web-lint` and `make fmt-check` check style.
- `make contracts` checks the generated OpenAPI and TypeScript.
- `make e2e` recreates the isolated browser server each run.
- `make verify` is the full suite. Run it before release work, not for every change.

[Development](docs/development.md) covers single tests, worktree projects and the Docker layout. Keep debug information and incremental compilation off in the tools containers.

## Test data

Tests use real temporary SQLite and owned, deterministic audio fixtures. Never copy the developer's library, titles, paths or credentials into a test, a fixture or a screenshot. Test external services against owned fakes at their protocol boundary; [testing](docs/testing.md#fake-plugins) describes the fake plugins.

## Verifying

- Use TDD for behavior changes. Write the failing test first, at the lowest public boundary that fails: capability, HTTP or browser.
- Preserve transaction, privacy, identity and recovery invariants when refactoring.
- Fix flaky tests at the root. Never raise a timeout, add a sleep, retry or skip to make a test pass. Wrap fixture setup in `hang_guard`, and keep behavior deadlines as plain timeouts.
- A green narrow test does not prove a release gate.
- UI changes need before and after screenshots from the isolated e2e server, at desktop and phone widths, in dark mode.
- Before accepting a slice, review it for security, correctness, performance and ownership.

## Commits and pull requests

- Make small, focused commits with single-line messages in plain language. Do not sign commits. Disable signing per commit without changing global Git settings: `git -c commit.gpgsign=false commit --no-gpg-sign -m "..."`.
- Open a pull request only when the developer asks.
- In the PR body, describe the problem in a sentence or two, then the fix and the evidence.
- UI changes need before and after images. Motion needs a short video. Do not commit PR-only screenshots.
- One request is one pull request.

## Documentation

`docs/` is written for people: what Hito does, how to run it and how it is built. Use plain language, and keep diagrams where they explain ownership or data flow. Say whether a statement is current behavior, planned behavior or an external dependency.

- Most code changes need no doc change. The code, types and tests record the implementation.
- Do not narrate control flow, list fields, keep file catalogs or append change summaries. When behavior changes, rewrite the affected text instead of adding a second account of it.
- Evidence logs, deployment receipts, review notes, handoff specs and checklists go in `.agents/meta/`. Tracked files never link to them.
- A personal deployment, such as the developer's own servers, is not product documentation.

## How it works

The server composes capability services from `hito-core` at startup: identity, catalog, artist catalog, playback access, collection, library storage, plugins, downloads, podcasts and replicas. HTTP handlers authenticate once through the shared extractor, call one capability and map typed results. Core owns every SQLite transaction, so the desktop host reuses the same rules without HTTP. Plugins are reached over HTTP. Hito resolves a recording to a source, then plays it through a relay that checks the account's access again. [Architecture](docs/architecture.md) has the diagrams.

| Path | Owns |
| --- | --- |
| `crates/core` | Identity, catalog, collection, indexing, plugins, SQLite and migrations |
| `crates/audio`, `crates/playback` | Native decoding and output, and the authorized queue |
| `crates/plugin-sdk` | The plugin wire contract (Apache-2.0) |
| `apps/server` | HTTP API, browser security, streaming and background workers |
| `apps/web` | The shared React UI |
| `apps/desktop` | The Tauri host |
| `apps/community` | The separate community ranking service |
| `plugins/` | Reference plugins |

## Taste

- Core capability services own their rules. Service composition has no business forwarding methods.
- SQLite, SQL, migrations, filesystem ownership and transaction rules stay private to core.
- HTTP adapters authenticate through the shared extractor, map typed requests and results, and own transport policy.
- OpenAPI and TypeScript are generated from the Rust routes and wire types. Never hand-maintain a second API model.
- A React page owns one route. Workflow hooks own async lifecycles and cancellation. Shared components take typed data and actions. Page rendering never subscribes to playback position ticks.
- Keep files cohesive and small. Split a file when it mixes workflows. Avoid empty wrapper layers, per-table repository traits and one-file-per-function fragmentation.
- Use clear names and explicit types. Write no code comments or doc comments; rename instead.
- Log every workflow with structured spans and events, safe outcomes, correlation IDs and useful timings. Frequent work belongs at debug or trace. Never log credentials, session material, raw bodies, audio, private paths, search terms, titles or signed URLs.
- If a rule here fights the task in front of you, say so and get the developer's sign-off before breaking it.
