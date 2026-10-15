# Development

Build and test Hito in Docker. The repository pins Rust 1.99.0 in `rust-toolchain.toml` and uses the same release in the Docker tools image. Rebuild the tools image after changing that pin:

```sh
docker compose --profile tools build rust
./scripts/verify.sh
```

The verification script checks formatting, Rust tests, Clippy, generated API contracts, browser unit tests, the production web build, dependency audit, and the isolated browser workflow. Its browser server uses temporary storage, separate from the normal server volume.

Browser fixtures live in `fixtures/e2e-generated`, distinct from development music in `fixtures/generated`. The verification script generates them with `audio-fixtures.mjs --e2e`. Do not mount personal music into the browser test server. Rust indexing tests use owned temporary WAV fixtures and protocol-boundary media-tool scripts, with FFmpeg supplied by the tools image.

## Rust editor support

Local editors need the pinned toolchain to load build scripts and procedural macros. Rustup selects it automatically inside this repository. Install it without changing your global default:

```sh
rustup toolchain install 1.99.0 --profile minimal --component rustfmt,clippy,rust-src,rust-analyzer
rustup show active-toolchain
```

The active toolchain should be `1.99.0`, overridden by this repository's `rust-toolchain.toml`. Restart the Rust language server after installing or changing the toolchain. Existing processes keep using their old macro helper until they restart.

Zed and VS Code project settings enable build scripts and procedural macros, run Clippy, and isolate rust-analyzer's build artifacts. Do not disable macro expansion to hide diagnostics. An error such as `proc-macro map is missing` can result from an incompatible compiler or macro helper. Inspect the language server log and compare its toolchain with the repository pin.

For an editor running entirely in Docker, open the provided `.devcontainer/devcontainer.json`. It uses the Compose Rust tools service and the same source tree, Cargo caches, and compiler as verification.

## Generated contracts

Rust handler registrations and wire types generate OpenAPI. The web application derives its TypeScript contracts from that schema:

```sh
docker compose --profile tools run --rm rust cargo run --quiet -p hito-server --example export_contract > docs/openapi.json
docker compose --profile tools run --rm node npm run generate:api
./scripts/check-contracts.sh
```

Commit both generated files with changes to the HTTP contract. Do not edit generated models by hand.
