# SQLx SQLite source provenance

`vendor/sqlx-sqlite` contains the published `sqlx-sqlite` 0.8.6 package. It is licensed under MIT or Apache-2.0; both upstream license files remain in the package.

- Published archive SHA-256: `c2d12fe70b2c1b4401038055f90f151b78208de1f9f89a7dbfd41587a10c3eea`, matching the original Cargo lockfile.
- Upstream commit recorded by the package: `bab1b022bd56a64f9a08b46b36b97c5cff19d77e`.
- Upstream directory: `sqlx-sqlite`.
- The initial vendor import preserves all 56 published files byte for byte. No other SQLx package is vendored.

The community-service startup regression demonstrates a lifetime gap during failed initial connection setup. SQLx can drop an initializing connection while its SQLite worker is still closing. Hito must retain its private state claim until that teardown finishes. A narrowly scoped worker-lifetime extension is being reviewed before activation; importing these sources alone changes no application behavior.
