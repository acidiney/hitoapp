# Listening history: pause and delete

Current behavior on the development tree. It runs on the local development server after a rebuild. It is not part of a public release, and nothing here has been checked on a deployed server.

## What history is

Listening history is the list of recordings an account has played, newest first. It feeds three things:

- the History page (`/history`, the newest 200 entries);
- *Recently played* and the *Unheard, near what you like* shelves on Home (up to twelve distinct albums taken from the newest 200 entries);
- nothing else. Likes, playlists, downloads and the resume position of a recording are not history.

Resume positions are a separate record. A recording's saved position and speed live in `listening_progress` and are what the player resumes from, what podcast episodes and audiobook chapters show as progress, and what the "heard" test on Home reads. They are written on every progress report whether history is on or off, and deleting history never touches them.

## Data

Migration 0085 adds two tables and copies the existing rows:

- `listening_history(user_id, recording_id, played_at)`, one row per account and recording, keyed on both, with an index on `(user_id, played_at DESC)`. The migration copies each `listening_progress` row to a history row with `played_at = updated_at`, so existing accounts keep the history they had. Recording and account deletion remove the rows (account deletion through the account data plan, recording removal through the list of tables that reference a recording).
- `account_history_preferences(user_id, history_enabled)`, with a check that the value is 0 or 1. An account with no row records history. The migration adds no rows, so no account is switched off by the upgrade. See [database maintenance](database-maintenance.md#migration-0085-listening-history).

## Writers

There is one writer. Every progress report, whether it comes from the browser player, the native player's progress writer or an offline device replaying `progress_set` through `POST /api/v1/sync`, is a `ProgressSet` operation applied by `Collection::apply`. In the same transaction that stores the position, it adds or refreshes the history row only if the account's preference is on. With the preference off, the position is stored and the history table is not touched, including the order of entries that already exist. A report that is replayed or arrives late uses the preference at the time it is applied, not the time it was played.

Community playback reporting does not read history. It has its own consent ([community service](community-service.md)) and its own queue, and pausing history neither enables nor disables it.

## Operations

| Operation | Route | Result |
| --- | --- | --- |
| Read the preference | `GET /api/v1/history/preferences` | `{"enabled": true}` |
| Change the preference | `PUT /api/v1/history/preferences` with `{"enabled": false}` | the new preference |
| Delete history | `DELETE /api/v1/history` | `{"removed": n}` |

All three need a session and answer `Cache-Control: no-store`. The request body of the change is closed (`deny_unknown_fields`), so an extra field or a non-boolean value is refused. Each operation checks the session inside its transaction and affects only the calling account. Delete removes every `listening_history` row of the account in one transaction and nothing else: progress, likes, playlists, downloads, sessions and other accounts are untouched. Pausing does not delete anything, and deleting does not pause. The History page's data stays `GET /api/v1/activity`; the History page also receives `history_enabled` with its page props.

Asking the browser to confirm is the client's job; the server deletes when asked.

## Web

Settings, Privacy & lock has a **Listening history** card:

- a switch, "Save listening history", that saves at once (it is a single, reversible choice, unlike the consent switches that keep Save and Cancel) and is disabled while a request is pending;
- a "Delete listening history" row whose button opens a confirmation dialog ("Delete listening history?", Cancel or Delete history). The dialog says what is removed and what is kept. After a successful delete the row reports how many entries were removed;
- a load or save failure is shown in place, with a reload action when the preference could not be read.

The History page shows "Start listening" when history is on and empty. When history is paused and empty it says "Listening history is paused" and links to the card. When history is paused and entries exist, they stay visible under a short notice. The card is also found by Settings search ("history", "recently played", "delete").

## Logging

Each change and each delete logs at info level the outcome, the account id, the new setting or the number of removed entries, and the elapsed time. The progress write logs at trace level whether a history row was written. Nothing logs recording ids, titles or paths. The browser logs the outcome, the setting, a count and the duration. Tests assert this in `crates/core/tests/history_privacy_logs.rs`.

## Tested evidence

- Core (`crates/core/tests/catalog/history_privacy.rs`): the default is on and per account; with history off a progress report stores the position but adds no entry and leaves existing entries and their order alone; turning it on resumes recording; delete removes only the caller's entries and keeps progress, likes and playlists; another account's entries and preference are untouched; Home's recently played follows history; a revoked session is refused by all three operations. `crates/core/src/collection/tests/home_work.rs` keeps the Home read within its work bound.
- Upgrade (`crates/core/tests/listening_history_upgrade.rs`): the previous schema objects are kept exactly, history is copied from progress, no preference row is added, foreign keys hold and the preference check refuses other values. The shared schema fixtures and the maintenance fixtures know the new tables.
- Account deletion: the deletion fixture turns history off before deleting, and the existing isolation test finds no remaining row for the account in any table.
- HTTP (`apps/server/tests/api/history_privacy.rs`): no session is refused with 401 on all three routes; pause and delete affect only the caller; the three responses carry `no-store`; invalid bodies are refused.
- Web: the settings card (switch, confirm, cancel, failures, account switch) and the History page empty and paused states pass as unit tests. One browser flow (`apps/web/e2e/history-privacy.spec.ts`) plays a track, pauses, plays another and checks it is not added, then deletes and checks that History and Home are empty.

## Not built

- Deleting one entry, or a date range.
- A retention period or automatic expiry.
- Hiding a single listening session ("private session").
- Syncing the setting to the desktop app's offline replica: the offline queue applies the server's current setting when it reconnects.
- A per-device setting. The setting belongs to the account.
