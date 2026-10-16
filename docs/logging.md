# Logging

Hito logs application operations locally. Logs are operational evidence, not community contributions. Community reporting remains a separate opt-in.

## Backend

HTTP requests carry a generated request ID returned in `X-Request-Id`. Structured events describe the matched route, HTTP method, result status, and elapsed time. Core spans record identity operations, catalog reads, synchronization acceptance/replay/conflict, scan leases and progress, and worker outcomes. Startup and shutdown are lifecycle events.

Use info for lifecycle and state transitions, debug for detailed operations, and trace for high-frequency events. Configure backend levels through `RUST_LOG`. Set `HITO_LOG_FORMAT=json` for JSON output. Compose enables JSON and retains three 10 MB log files per normal server container.

```sh
docker compose logs --tail=100 server
```

Input extractors intercept default Axum rejections before they can log submitted values. They retain HTTP statuses and emit static error categories. Trace capture tests include invalid numeric passwords, private action kinds, malformed queries, body-read errors, and oversized input.

Media inspection logs cache reuse, verified evidence, unsupported profiles, command failure categories, skipped-file counts, legacy adoption counts, and source replacement. Its subprocess runner rejects error diagnostics without retaining or logging their text. Directory and file errors become static categories before entering instrumented results. Scan leases continue to emit progress while verification is running.

The standalone desktop host writes structured JSON to stderr. Events cover startup, private window bootstrap, access/navigation rejection, backend readiness, interrupted-scan requeue, command cleanup, forced HTTP drain, and joined application shutdown. Bootstrap query rejection uses a static message before Axum can render or trace submitted fields. A TRACE regression checks private query sentinels and the ticket. Bootstrap tickets and process cookies must never be logged. Desktop file rotation and user-facing diagnostic export remain release work.

The native audio engine adds decoder readiness, transport controls, output startup/cleanup, typed worker failures, and underrun recovery. Its spans use a process-local audio ID. Device callbacks update counters without logging. Vendor diagnostics are suppressed around Symphonia calls because unknown media chunks can otherwise enter logs. A trace-level regression retains Hito's readiness event while rejecting a media-byte sentinel.

The native playback owner adds input opening/ranges, cancellation, readiness, revocation/failure, and joined session cleanup. A process-local numeric session ID correlates async supervision, blocking controls, and the native worker. Spawned work retains the tracing subscriber as well as its span. An actual device workflow captures TRACE logs, requires the supervisor and engine cleanup events, and rejects session-token, credential, and private-path sentinels.

Ordered recording selection logs only account ID and batch count. Retained-session validation uses trace events with safe error categories. Provider-source refresh emits debug events for cancellation before publication and write admission, followed by its existing safe completion or failure. It logs no source-page body or credentials. The SQLite publication fixture observes the write event under an account-scoped capture of the global subscriber.

The in-process command/queue owner adds a numeric `player_owner` correlation, account and command IDs, intent sequence, static action names, accepted revision, request outcome, elapsed milliseconds, session withdrawal, and joined admission shutdown. Frequent requests use debug spans/events. Queue snapshots, metadata, lease values, and tokens are never logged. Its JSON capture test verifies correlation through the retained supervisor and rejects private title, lease, credential, and filesystem sentinels. This owner is not connected to native output or HTTP yet.

## Browser

The browser logs API outcomes, request IDs, user actions, navigation, scan state, and playback commands, source changes, recovery, and failures. Grid/list changes, settings sections, and expanded-player/queue visibility use debug events with static modes. Frequent events use debug. Browser logging uses the local developer console and sends no external telemetry. Set `localStorage["hito-log-level"]` to `debug`, `info`, `warn`, `error`, or `off`, then reload. The default is `info`.

Provider discovery logs HTTP outcomes, result counts/page indexes, and completed save/play actions with durable recording IDs. Playback selection preparation and supersession use debug. Queries, opaque pagination cursors, remote recording/source IDs, provider titles, and rejected response messages stay out of these events. A workflow regression captures successful search plus failed adoption and checks those private sentinels.

## Redaction

Log semantic operation names, opaque object IDs where useful, counters, durations, and safe error categories. Exclude passwords, authentication tokens, cookies, raw request/response bodies, signed audio URLs, filesystem paths, titles, and search terms. An error message can contain a private path, so logging a raw error object is not automatically safe.

Capture tests verify that successful and failed operations are observable and that known sentinel secrets and private paths never appear. Browser logging filters fields through an allowlist and never passes raw error objects to the console.
