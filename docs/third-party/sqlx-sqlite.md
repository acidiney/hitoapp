# SQLx SQLite source provenance

`vendor/sqlx-sqlite` contains the published `sqlx-sqlite` 0.8.6 package. It is licensed under MIT or Apache-2.0; both upstream license files remain in the package.

- Published archive SHA-256: `c2d12fe70b2c1b4401038055f90f151b78208de1f9f89a7dbfd41587a10c3eea`, matching the original Cargo lockfile.
- Upstream commit recorded by the package: `bab1b022bd56a64f9a08b46b36b97c5cff19d77e`.
- Upstream directory: `sqlx-sqlite`.
- The initial vendor import preserves all 56 published files byte for byte. No other SQLx package is vendored.

## Local worker-lifetime extension

The workspace activates this package through a Cargo patch. Three upstream files add an optional `SqliteConnectOptions::worker_guard` factory, reserve its guard before thread creation, and transfer that guard into the worker. The default is absent. No other SQLx package is patched.

The community-service startup regression demonstrates a lifetime gap during failed initial connection setup. SQLx can drop an initializing connection while its SQLite worker is still closing. The guard lets Hito retain its private state claim through worker-local cleanup, including failed startup. It does not join the OS thread or prove that all external owners of shared SQLite state have released it. The community lifecycle closes the pool to size zero before waiting for its guard count. See [ownership and teardown](../community-service.md#private-state-and-teardown).

The real held-close regression, normal HTTP restart/replay tracer and unsafe-ancestry regression pass in Docker. Compatibility checks against the patched package pass for twelve selected core cases, all 38 folder-plugin cases and five server lifecycle/download cases. Strict all-target Clippy passes for those packages and the community crate. These checks do not establish full workspace, packaged-image, platform or public-release acceptance.
