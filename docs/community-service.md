# Community service

`hito-community` owns an independent SQLite store and Axum router for reporter admission, successful-playback receipts, and candidate rankings. It has no dependency on the application core or its database. Application opt-ins, corrections, and public operation remain unfinished under the [release plan](release-plan.md#community-service).

## Current HTTP contract

POST request bodies use JSON with a 16 KiB limit. Request objects reject unknown fields. Authentication accepts one `Authorization: Bearer` header containing a 64-character lowercase hexadecimal token. Responses carry `Cache-Control: no-store` and a generated `X-Request-Id`.

| Route | Authority | Behavior |
| --- | --- | --- |
| `POST /v1/admin/reporters` | Separate administrator bearer | Accepts `{}` and returns `201` with a new reporter UUID and random 256-bit bearer. The token is returned on creation; there is no token-read endpoint. |
| `POST /v1/outcomes` | Provisioned reporter bearer | Accepts a version-4 submission UUID, public source reference, and `successful_playback` outcome. A committed new receipt returns `201`; an exact retry by that reporter returns the original receipt with `200`. Reusing its UUID for another source returns `409`. |
| `POST /v1/rankings` | Public | Accepts 1–32 distinct candidate source references and returns only those candidates, ordered by distinct successful reporters. Equal counts preserve input order. |
| `GET /v1/openapi.json` | Public | Returns OpenAPI 3 generated from the registered Rust handlers and actual wire DTOs, including distinct administrator and reporter bearer schemes. |

The schema records strict input objects, the candidate count and uniqueness rules, and request and response types. Field descriptions explain the conditional provider-ID rules and version-4 UUID requirement without changing accepted UUID text forms. The candidate schema uses the actual `SourceRef` component and shares its maximum count with runtime validation. There is no separately maintained JSON API model.

The [Rust wire types](../crates/hito-community/src/wire.rs) define the contract. Source references contain `provider` and `recording_id`. YouTube IDs have exactly 11 ASCII letters, digits, underscores, or hyphens. Jamendo IDs are canonical positive decimal `u64` values without leading zeros. Other providers and URL-shaped references are rejected. No request field accepts audio, paths, credentials, stream URLs, titles, or arbitrary metadata.

The trusted host supplies an administrator token and an absolute state directory through `Config`. Only the administrator can provision reporters. The database stores reporter-token hashes; the administrator-token hash stays in process memory. A reporter UUID is a service pseudonym, separate from application accounts and backend host or synchronization identities. Credentials do not attest real listening or distinct humans. The service accepts a playback-success claim; it does not validate audio or measure playback itself. Selection reports are not an accepted outcome.

Each reporter contributes at most one count per source, even with multiple submission UUIDs. Counts cover the currently retained reports. They are not a measured playback-success probability. The future application client must filter candidates to configured, available providers before requesting rankings and preserve personal source choices and overrides. Sending candidate IDs is itself a disclosure that the opt-in design must address.

## Private state and teardown

```mermaid
flowchart LR
  HTTP[Thin authenticated HTTP adapters] --> Capabilities[Admission / Outcomes / Rankings]
  Capabilities --> Pool[Private SQLx pool]
  Pool --> DB[(Separate SQLite / WAL)]
  Lifecycle[Retained startup and shutdown] --> Owner[StateOwner / private directory and lock]
  Owner --> DB
  Pool --> Factory[Optional worker-guard factory]
  Factory -. Weak reference .-> Owner
  Factory --> Tracker[Closing gate and worker count]
  Worker[SQLx worker] --> Lease[WorkerLease]
  Lease --> Owner
  Lease --> Tracker
```

Capability modules own SQL and transactions. The service composition owns startup and shutdown. State admission checks the original ancestry component by component, including alias locations and targets. Ancestors must belong to root or the effective user and prevent replacement by other users; sticky temporary directories are allowed. State directories and files must belong to the effective user with exact `0700` and `0600` modes. Final entries use no-follow opens. Linux SQLite paths use the retained directory handle. Non-Unix state admission currently fails closed.

The pinned [SQLx SQLite package](third-party/sqlx-sqlite.md) has an optional worker-guard factory. Its default is absent, so other consumers keep the existing behavior. A guard is reserved before thread creation and transferred to the worker. It is released after worker-local resources and references are released. Shared SQLite state can also have owners outside that worker, so the guard is not a promise of complete SQLite destruction or OS thread termination.

The community factory holds the tracker and a weak state-owner reference. Under one mutex it checks that admission is open, upgrades the owner, and increments the worker count. A worker lease releases its owner reference before publishing the decremented count. Shutdown closes admission, closes the pool to size zero, waits for counted leases to finish, releases the state owner, then acknowledges completion. Pool options and live routers do not keep a strong state-owner reference. Failed initial connection setup waits for the same lease barrier; retained startup work survives caller cancellation.

SQLx core 0.8.6 can return from `Pool::close()` after a concurrent connection return has repopulated its idle queue. A root Docker diagnostic observed pool size 1 and idle count 0 before close, then size 1 and idle count 1 after it returned. That idle connection retained its worker sender. The private teardown therefore awaits close passes until size reaches zero before waiting for worker leases. With the configured one-connection pool and admission closed, an unfinished connection must either close before releasing its permit or enter the idle queue before releasing it. A second pass drains that late return. This barrier uses awaited resource cleanup rather than timed polling.

Structured events contain static routes, safe outcomes, request IDs, counts, and durations. Token, reporter, source-reference, path, raw-body, and raw-driver-error values are excluded from the service events. Dedicated capture and adversarial privacy tests remain pending.

## Evidence and remaining work

Root-run Docker checks after the teardown correction passed:

| Public regression | Evidence |
| --- | --- |
| [Outcome HTTP tracer](../crates/hito-community/tests/outcome_http.rs) | Reporter provisioning, durable receipt, restart, exact replay, and candidate ranking passed in 0.03 seconds. Close runs while the router still owns service clones. Each phase has a bounded deadline. |
| [Failed startup](../crates/hito-community/tests/startup_failure.rs) | An actual SQLite authorizer rejects initial journal-mode setup and a destructor holds real close. Startup remains pending and retains its state lock until release, then recovery succeeds. Passed in 5.04 seconds. |
| [State admission](../crates/hito-community/tests/state_admission.rs) | A private state directory below a nonsticky `0777` ancestor is rejected before state files are created. Passed in less than 0.01 seconds. |
| [Generated OpenAPI](../crates/hito-community/tests/openapi.rs) | The public document describes the four actual routes, distinct authority, strict DTO references, body and ID constraints, candidate bounds, replay/conflict responses, and fixture-secret/path exclusion. Passed in 0.02 seconds. |

All four community cases and three Jamendo cases, including its real-process fixture, pass together in a locked Docker run. One independent source review of the OpenAPI slice found no issues. Workspace formatting and strict all-target Clippy for community and Jamendo pass after the test-table readability correction.

The preceding foundation passed workspace and modified-driver formatting, strict all-target Clippy for community, core, server and the folder plugin, and 55 directed driver compatibility cases. Those comprise twelve selected core cases, all 38 folder-plugin cases and five server cases covering embedded lifecycle, signals and download restart. These are development checks, not full workspace, packaged-image or platform acceptance. Caller-cancellation, setup-timeout, spawn-failure, wider alias and ownership cases, and load behavior need their own controlled evidence.

The service is not ready for public deployment. Rate limits, quotas, retention and erasure, reporter revocation, moderation, correction suggestions, provider-backed validation, binary bootstrap, deployment configuration, hosting, and DNS remain open. Correction suggestions must require provider validation or maintainer approval before affecting automatic matching. Separate disabled-by-default application opt-ins, account isolation, personal overrides, and playback during community outages remain client work. There is currently no application-to-community network path and no outbound provider fetch in this crate.
