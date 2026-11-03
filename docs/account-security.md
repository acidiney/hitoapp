# Account security: password change and open sessions

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

- `GET /api/v1/account/sessions` lists the account's own unexpired sessions (at most 200, the current one always included), newest first: `{"id", "created_at", "current"}`. `created_at` is Unix seconds. The list shows nothing of other accounts.
- `DELETE /api/v1/account/sessions/{id}` revokes one session (`204`). An id that is unknown or belongs to another account answers `404`. Revoking the current session ends it like sign out and clears the cookie.
- `DELETE /api/v1/account/sessions` ends every other session and keeps the current one. The response is `{"revoked": n}`.

### Session id

The id is the first 128 bits of SHA-256 over a fixed label and the stored token hash. The stored token hash is itself a SHA-256 of the cookie or bearer value. The id cannot be turned back into either one and is never equal to them, and the cookie or token is never returned. It is stable for the life of the session, so no migration and no new column are needed. Revoking looks the id up among the caller's own sessions only.

### Not recorded

The session table stores neither a client description nor a last-seen time, so the list shows only when each session was created (expiry minus the fixed 30 day lifetime, one constant shared by sign-in, this list and the cookie). Recording them would need a migration and a write on requests; it is listed in the [TODO](../TODO.md).

## Web

Settings, Profile has a Change password card and an Open sessions card. A wrong current password (`wrong_password`) shows "The current password is not correct." and keeps what was typed for the new password; `429` adds that unknown sessions can be revoked under Open sessions; other failures, including `409`, show the server message. After a change the list is read again. Each other session has Revoke; Sign out other sessions is disabled when there are none. Both hooks cancel their requests when the page or account changes.

## Logging

Events: `password_confirmation_rejected`, `password_changed` (with the number of sessions ended), `session_revoked`, `other_sessions_revoked`, plus the usual request events. No password, hash, token or session id is logged. A server test captures TRACE output and rejects password, hash and token sentinels.

## Evidence

Core cases (`crates/core/tests/account_sessions.rs`), HTTP cases (`apps/server/tests/api/account_sessions.rs`), a logging case (`apps/server/tests/account_session_logging.rs`) and web cases (`apps/web/src/features/session/tests/account-security.test.ts`). No browser test covers the cards yet.
