# Account security: password change, open sessions and display name

Current behavior on the development tree. It runs on the local development server after a rebuild. It is not part of a public release.

## Change password

`POST /api/v1/account/password` with `{"current_password", "new_password"}`.

- The new password follows the same rule as first-run setup: 12 to 1024 bytes (`422` otherwise). The same Argon2 hashing is used.
- A wrong current password answers `422` with the error code `wrong_password`, the same as the PIN routes, and changes nothing. `401` is kept for an invalid or expired session, so a client that drops its token on `401` is not signed out by a typing mistake.
- If another change of the same account commits first, this one answers `409` ("Your password was just changed elsewhere. Sign in again.") and changes nothing. The hash is updated only if it still equals the hash that was verified.
- On success the account's other sessions are deleted in the same transaction that stores the new hash, and the session that made the request stays signed in. The response is `{"revoked": n}`. Other accounts are untouched.
- A sign-in that verified the old password but had not yet created its session when the change committed fails with `401`: the session is inserted only if the verified hash is still the current one. No session from before the change survives.
- The change is refused with `423` while the session is screen-locked ([screen lock](screen-lock.md)).

### Rate limiting

Two limits apply, so a stolen session cannot be used to guess the password:

- Per account (core), one shared budget: at most 5 wrong passwords in a 15 minute window across every route that asks an authenticated account for its password: change password, set PIN and remove PIN. The attempt is counted before the password is checked, so parallel guesses cannot exceed the limit. After that those routes answer `429` with `Retry-After` until the window ends, even for the correct password. A correct password clears the count. The count is kept in memory, so a restart resets it. Sign-in has its own per-address limit and is unaffected.
- Per address (HTTP): 10 requests a minute on each of these routes, with the same limited Argon2 slots as sign-in.

A short new password is refused before the current password is checked, so it neither counts nor reveals anything.

## Open sessions

- `GET /api/v1/account/sessions` lists the account's own unexpired sessions (at most 200, the current one always included), newest first: `{"id", "created_at", "last_seen_at", "client", "current"}`. `created_at` and `last_seen_at` are Unix seconds. `client` is a coarse label such as "Firefox on macOS", or null for a session that was created before the label existed. The list shows nothing of other accounts.
- `DELETE /api/v1/account/sessions/{id}` revokes one session (`204`). An id that is unknown or belongs to another account answers `404`. Revoking the current session ends it like sign out and clears the cookie.
- `DELETE /api/v1/account/sessions` ends every other session and keeps the current one. The response is `{"revoked": n}`.

### Session id

The id is the first 128 bits of SHA-256 over a fixed label and the stored token hash. The stored token hash is itself a SHA-256 of the cookie or bearer value. The id cannot be turned back into either one and is never equal to them, and the cookie or token is never returned. It is stable for the life of the session, so no migration and no new column are needed. Revoking looks the id up among the caller's own sessions only.

### Client label and last seen

Migration 0082 adds `created_at`, `last_seen_at` and `client` to `sessions`. Existing rows get `created_at` and `last_seen_at` equal to the old expiry minus the 30 day lifetime and a null label. See [database maintenance](database-maintenance.md#migration-0082-session-client-and-last-seen).

- Label: sign-in and first-run setup derive it from the `User-Agent` header in the HTTP adapter (`apps/server/src/security/client_label.rs`, a pure function with unit tests) and pass only the label to core. It is a browser family plus an operating system family: Edge, Opera, Samsung Internet, Firefox, Chrome or Safari, on iPhone, iPad, Android, ChromeOS, Windows, macOS or Linux. A client that matches nothing is "Unknown browser"; one that sends the `HitoDesktop/` token is "Hito desktop". The raw header is never stored, logged or returned, and no IP address is stored. Core also strips control characters and cuts the label to 64 bytes, and the column has a check for the same bound. The desktop shell sends the token: its window sets a User-Agent made of a fixed platform part and `HitoDesktop/<crate version>`, built by one function (`user_agent` in `apps/desktop/src/user_agent.rs`) that the window and the tests both use. Desktop sign-ins therefore show as "Hito desktop". The platform part is a fixed per-system string, not the WebView's own, because Tauri can only replace the User-Agent, not extend it.
- Last seen: every request that authenticates a session (cookie or bearer, locked or not) reads the idle time in the query it already runs. Only when that is 300 seconds or more does it run `UPDATE sessions SET last_seen_at = now WHERE token_hash = ? AND now - last_seen_at >= 300`. That condition makes parallel requests write at most once. So the extra cost is one subtraction per request and one single-row write per session every 5 minutes, with no in-memory state to lose on restart or to share between processes. The write waits at most 250 ms; if it fails or times out, the request still succeeds and a debug event `session_seen_failed` with a static `error_kind` is logged. A rejected or expired session never writes. Only the authenticating session is updated, never another session or account.

### Last active in the administrator account list

`GET /api/v1/users` (administrators only; everyone else gets `403`) returns `AccountSummary` rows: `id`, `username`, `admin` and `last_active_at`. `last_active_at` is the newest `last_seen_at` among that account's sessions that have not expired, as unix seconds, or `null` when the account has no open session (never signed in, or every session expired). Because last seen is written at most every 5 minutes, the time can be up to 5 minutes behind. The list shows nothing else about sessions: no ids, no client labels, no per-session times. Other views of an account (`/me`, sign-in, account creation) do not carry `last_active_at`. The rows also carry `display_name` (null when unset), see [display name](#display-name).

## Display name

An account can have an optional display name next to its username. The username is still the only sign-in identity: sign-in, the account deletion confirmation and the first-run form use it, and the display name is never matched against anything.

- Storage: migration 0086 adds `users.display_name`, null by default, with a check for 1 to 64 characters. Existing accounts keep null. See [database maintenance](database-maintenance.md#migration-0086-account-display-name).
- `PUT /api/v1/account/display-name` with `{"display_name": "..."}` sets the name of the caller's own account and answers `200` with the account (`User`). A null or blank value clears it. There is no route that changes another account's name, and administrators have no edit capability for it; they only see it.
- Rules (core): the value is trimmed, then must be 1 to 64 Unicode scalar values (not bytes, so emoji and accented letters count once). Control characters, line breaks, tabs and invisible format or bidirectional characters (zero-width space, bidi overrides and isolates, soft hyphen, the byte order mark and similar) are refused with `422`. A zero-width joiner is accepted only between two symbols, so emoji sequences such as a person with a skin tone and an occupation keep working while a joiner inside a word does not. Names are not unique; two accounts can share one. A locked session answers `423`.
- Where it appears: `GET /me`, sign-in and account creation return `display_name` (null when unset), `GET /users` rows carry it for administrators, and the shared page props `user` carries it, so the web gets the new name after a partial reload of `user`.
- Logging: the change logs `display_name_changed` with only whether a name is now set, never the name. The web logs the same outcome with a count of 0 or 1.

## Web

Settings, Profile has a Display name card (a text field, a Save display name button that stays disabled until the text changes, a message after saving, and the server's message on refusal; the 64 character limit is also checked before sending). The account menu, the Profile header, the Accounts rows, the Home greeting and the avatar initials use the display name and fall back to the username. When a display name is set, the Profile header and each Accounts row show the username beside the role as secondary text. Initials come from the display name; a name made only of symbols uses its first symbol. The name is always rendered as text, so markup typed into it is shown literally. "Signed in as" in About keeps showing the username. First-run setup and Create account do not take a display name; it is set afterwards in Profile.

Settings, Profile has a Change password card and an Open sessions card. A wrong current password (`wrong_password`) shows "The current password is not correct." and keeps what was typed for the new password; `429` adds that unknown sessions can be revoked under Open sessions; other failures, including `409`, show the server message. After a change the list is read again. Each row reads like "Firefox on macOS · Last active 5 minutes ago", with the sign-in time below it and "This session" marking the current one. Each other session has Revoke; Sign out other sessions is disabled when there are none. Both hooks cancel their requests when the page or account changes. Settings, Accounts shows each account's hint as "Administrator · Last active 5 minutes ago" or "Member · No open sessions".

## Logging

Events: `password_confirmation_rejected`, `password_changed` (with the number of sessions ended), `session_revoked`, `other_sessions_revoked`, plus the usual request events and the debug event `session_seen_failed` when the last-seen write fails. No password, User-Agent, IP address, hash, token or session id is logged. A server test captures TRACE output and rejects password, hash and token sentinels.

## Evidence

Core cases (`crates/core/tests/account_sessions.rs`), the 0082 upgrade (`crates/core/tests/session_client_upgrade.rs`), parser cases (`apps/server/src/security/client_label.rs`), HTTP cases (`apps/server/tests/api/account_sessions.rs`), a logging case (`apps/server/tests/account_session_logging.rs`) and web cases (`apps/web/src/features/session/tests/account-security.test.ts`). The last active column has core cases (`crates/core/tests/account_last_active.rs`), HTTP cases (`apps/server/tests/api/identity.rs`) and web cases (`apps/web/src/features/accounts/tests/account-last-active.test.ts`). No browser test covers the cards yet.
