# Artist matching

Hito can now group recordings under one reviewed artist without a user clicking through every track, but only when the data itself names that artist. Everything else stays a suggestion that the account accepts or dismisses. Since the move to automatic matching below, a background job does the walking. Equal names alone never merge artists: the one automatic use of a name is a MusicBrainz search that must be corroborated by the artist's own recordings.

This page separates what runs today from what is planned. Current behavior is implemented in core, the authenticated API and the shared web app. It runs on the local development server (migrations through 0071), which is not a public release. Matching from provider evidence (ISRC, artist pages and name searches corroborated by recordings) is covered by owned protocol fakes only: live acceptance remains blocked by upstream HTTP 503 answers; see [what is verified](#verification) and [open work](#open-work).

## Two kinds of evidence

| Evidence | Example | Effect |
| --- | --- | --- |
| Verified | One MusicBrainz performer artist identifier embedded in a local file's own tags | Applied automatically as a reviewed association and a chosen artist |
| Verified through MusicBrainz | An ISRC stated by a provider or a file tag that MusicBrainz maps to exactly one recording credited to exactly one artist | Applied automatically with the same rules; provenance `matched_isrc` |
| Verified through MusicBrainz | A lone credited artist page of an imported track that MusicBrainz links to exactly one artist | Applied automatically with the same rules; provenance `matched_provider_url` |
| Corroborated through MusicBrainz | A credit name that exactly one MusicBrainz artist carries, whose own recordings share at least two titles and lengths with the credit's undecided library tracks | Applied automatically to the corroborated tracks only, with the same rules; provenance `matched_name_discography` |
| Credit-level | Any of the rows above for some tracks of a credit, with a clear majority artist | The credit's other undecided tracks that carry no evidence of their own receive the same artist; provenance `matched_credit_evidence` |
| Uncertain | The same credit name, a shared album title, a track with the same title and length | Ranked suggestion only; nothing changes until the account confirms |

```mermaid
flowchart TD
    Page[Artists page opens] -->|silent, cancellable, local only| Run[Match from file tags, batches of at most 100]
    Run --> Policy{Built-in MusicBrainz selected and enabled?}
    Policy -->|no| Nothing[Nothing is matched]
    Policy -->|yes| Tags[Current observation: exactly one distinct valid performer id?]
    Tags -->|none or several, or not an identifier| Skip[Left alone, counted as ambiguous]
    Tags -->|one| Decided{Any earlier personal decision?}
    Decided -->|choice, reset, split, unlink, personal artist edit, hiding shared correction| Declined[Left alone, counted as declined]
    Decided -->|no| Apply[Reviewed association plus chosen artist, provenance matched from file tag]
    Apply --> Group[Same id joins the same chosen artist group]
    Credit[Credit page opens] --> Suggest[Chosen artists carrying the same credit name]
    Suggest --> Rank[Rank by shared albums, then same-length tracks]
    Rank --> Choice{Account decides}
    Choice -->|Review suggestion| Existing[Existing apply-to-credit review path]
    Choice -->|Not the same artist| Dismiss[Remembered per account and credit]
```

## Evidence sources and the match decision

```mermaid
flowchart TD
    Tag[File tag: one performer MusicBrainz id] --> TagPass[Tag pass: no network]
    Isrc[ISRC from a provider response or a file tag] --> Lookup
    Page[Provider response: exactly one credited artist page] --> Lookup
    Lookup[Provider pass: bounded, paced, cancellable MusicBrainz lookups] --> Answer{Exactly one recording, exactly one credited ordinary artist?}
    Answer -->|no, zero, several or Various Artists| Uncertain[Stays separate and is remembered until the observation changes]
    Answer -->|MusicBrainz unreachable| Retry[Not remembered, asked again by a later pass]
    Answer -->|yes| Same
    TagPass --> Same{Earlier personal decision?}
    Same -->|choice, reset, split, unlink, personal edit, hiding correction| Left[Left alone and not looked up]
    Same -->|none| Apply[Reviewed association plus chosen artist with provenance]
    Apply --> Group[Same artist id joins the same chosen artist group]
    Evidence[Evidence cursor exhausted] --> Names[Name phase: credits with at least two undecided tracks]
    Names --> Search[Artist search by exact credit name, 100 results with scores]
    Search --> Complete{Does the page hold every score-100 result?}
    Complete -->|no| Uncertain
    Complete -->|yes| Browse[Exact-name artists, at most three: page through recordings until two titles match, at most five pages each]
    Browse --> Corroborate{Exactly one artist shares two or more titles with lengths within two seconds?}
    Corroborate -->|no, several, or a namesake not fully read| Uncertain
    Corroborate -->|MusicBrainz unreachable| Retry
    Corroborate -->|yes, for the tracks that share| Same
    Same -.->|after the individual matches| Credit[Credit-level step: the credit's other undecided tracks with no evidence of their own follow]
    Credit --> Group
```

The provider pass (`POST /api/v1/artist-matching/provider-evidence`, core `match_artists_from_provider_evidence`) is started by the Artists page when it opens, but only while the account's metadata settings select the built-in MusicBrainz provider (see [consent](#consent-for-provider-lookups)). The ISRC decides first; the artist page is consulted only when the ISRC names no single artist. Everything below the decision node is shared code, so both passes obey the same personal-decision rules.

## Verified matching from file tags

`POST /api/v1/artist-matching` (core `match_artists_from_tags`) examines recordings of libraries the account can read, in recording-identifier order, at most 100 per request. Each request carries a request identifier and an optional cursor. A request that matched something stores an immutable receipt and replays exactly, including the stored `next_after` cursor, so a replay never shows newer progress; the same identifier with different contents conflicts. A request that matched nothing stores nothing, so the Artists page cannot grow the receipt table.

A recording qualifies only when all of these hold:

- Its current observation comes from a file and retains performer artist tag assertions (see [tag evidence](metadata.md#musicbrainz-artist-tag-evidence-in-verified-source)).
- Those assertions contain exactly one distinct value, and that value is a single MusicBrainz identifier. A value with a delimiter (several artists in one tag), a malformed value, two different identifiers, or only album-artist assertions is not evidence. Ambiguous assertions are counted and ignored.
- The account has no choice row for the recording. A reset, a split back to the credit, a retired choice and an existing different choice all leave a row, so none of them is overridden. The one exception is a stale automatic match: a choice written by an earlier pass (its receipt order equals the provenance row's) that is retired or points at an observation that is no longer current, which no later account decision has replaced. After a retag, such a recording is matched again from its new tags; any reset, split, choice or edit made by the account has a different receipt order and stays untouched.
- The recording's artist has no personal edit. A personal artist edit means the tag no longer describes the credit.
- The account has not unlinked that association, and no applied, unretired administrator correction names that recording, observation and artist. The correction check does not depend on the account having an association, so a silent pass by another account cannot undo an administrator's "this id is wrong here".
- The account's metadata enrichment is enabled and includes the built-in provider. Disabling enrichment stops matching. The pass reads no network: it needs no MusicBrainz request, so provider outages cannot affect it.

Candidates are selected with a read, so scanning untagged or ambiguous files never holds the write lock. Only the apply batch runs in one immediate transaction that re-validates every decision above. A match runs the same steps as the bulk apply: it retains a reviewed association for the recording's own credit and observation, creates the account's chosen-artist group for the built-in provider and identifier if it does not exist, and stores the choice at its next revision with the receipt's acceptance order. The association keeps `user_confirmed` evidence only when the account had already reviewed that artist; a new association carries the provenance row `file_tag_artist_id`, returned by the association API as evidence `matched_file_tag_artist_id`. The shared UI does not display it yet. Provenance rows are immutable.

Two differently spelled credits with the same identifier therefore land in one group, and a credit leaves the Library credits list once the group covers it (see [group browsing](artist-identity-groups.md#group-browsing)). Two different identifiers sharing a name stay in two groups. Recordings, sources, playlist entries, likes, progress and personal edits keep their identifiers and values; matching writes only associations, choices, a group and a receipt.

Recordings that were examined and not matched (an ambiguous or valueless tag, a personal artist edit, an unlinked association or a hiding correction) are remembered in `artist_match_checks` with the observation and personal artist operation that were checked. They are not examined again until the observation or the personal artist edit changes, so a second pass over an unchanged library reads no candidates and takes no write lock. A recording that could not be read at that moment (its credit was not current) is not remembered. A later withdrawn shared correction or relinked association is therefore not noticed until the observation changes. A request is bounded by the standard 15-second budget and answers a conflict if it is exceeded; the work is resumable by cursor.

## Matching from provider evidence

Recordings carry two kinds of provider evidence. Both are stored as immutable rows beside the observation that stated them, so a later observation can never inherit an older answer.

- ISRC. A provider states it on a recording (`ProviderRecording.isrc`; see the [plugin contract](plugin-catalog.md#recording-evidence)); a local file states it in a tag, retained since inspection version 4 as `InspectedTags.isrc` (ID3 `TSRC`, Vorbis `ISRC` and MP4 `----:com.apple.iTunes:ISRC`, read through FFprobe as `TSRC`, `ISRC` and `ISRC`; the raw value is kept like any other tag). The code is normalized (case and the hyphenated display form) and stored in `recording_isrc_observations` with its origin, `file_tag` or `provider_response`. A malformed code is ignored and never fails an import or scan.
- Artist pages. A provider may list the canonical pages of every credited performer (`ProviderRecording.artist_urls`). Only an allowlisted shape is accepted: `https://open.spotify.com/artist/` followed by exactly 22 base62 characters. The allowlist is a small typed list in the plugin SDK (`artist_page_provider`), enforced when a plugin response is validated, when pages are stored, when the MusicBrainz client validates a request, and again before each lookup, so rows from earlier versions with any other address are never looked up. Another provider is added by adding one shape. Accepted pages are stored in `recording_artist_url_observations`. Only a recording that lists exactly one page is a candidate, because two pages would not say which artist a one-artist group should receive.

A candidate is a recording of a library the account can read whose current observation has such evidence, which the account has not yet decided about and which no earlier provider pass settled for the same observation and personal artist operation. Candidates are read in recording-identifier order. The pass then asks the built-in MusicBrainz client:

- `GET /ws/2/isrc/<ISRC>?inc=artist-credits` and requires exactly one returned recording whose credit names exactly one distinct artist. A 404 is an empty answer.
- `GET /ws/2/url?resource=<page>&inc=artist-rels` and requires exactly one related artist. A 404 is an empty answer.

The special-purpose artists Various Artists, [unknown], [no artist] and [anonymous] never count as the one artist. Names, titles and durations are never compared in this phase; the code or the page is the evidence. A recording with several credited artists, several recordings sharing a code, a code MusicBrainz does not know, or a page linked to several artists is *uncertain*: nothing changes, it stays separate, and it is not examined again until its observation changes. MusicBrainz may add data later; Hito does not poll for that yet.

Bounds. One request examines at most 100 recordings but performs at most eight MusicBrainz lookups and checks the nine-second window and the three-consecutive-failure limit before each lookup, not only between recordings; a lookup still running 13 seconds after the pass started is abandoned as unavailable, so the pass ends inside the 15-second request budget. When the window closes during the first recording of a batch, its remaining lookup counts as unavailable (retried later); when it closes during a later recording, that recording waits for the next batch. The pass returns a cursor so the web app (or any client) continues. Every lookup goes through the shared one-request-per-second MusicBrainz gate, and the request is admitted through the same per-account and global slots as other metadata work. Dropping the request (a closed page) cancels the lookup in flight and writes nothing. A code asked in the same request is asked once; a code shared by recordings in different requests can be asked twice. MusicBrainz outages, timeouts, oversized or invalid answers are *unavailable*: they are counted, not remembered, and retried by a later pass. Disabled enrichment (or a selection without the built-in provider) makes the pass do nothing and ask nothing.

What is sent by the evidence phase. Only the normalized ISRC or the canonical artist page address, with the Hito user agent. No account, title, credit name, path, library or token leaves the host; the name phase below is the only step that sends a name. A deployment that must not contact MusicBrainz turns metadata enrichment off for the account, which stops the pass before any lookup.

## Matching by name with corroboration

A credit name is never evidence by itself. After the ISRC and artist page candidates are exhausted, the same pass continues into a *name phase*: the cursor `next_after` becomes `name:` (then `name:<credit identifier>`). The background job and the manual **Match again** action walk it with the same cursor and the same cancellation. Nothing else asks MusicBrainz for a name: page load, the tag pass and suggestions never do.

Which credits. A credit (the account's artist row in a library) is examined only when at least two of its recordings are undecided: no personal choice, reset, split, unlink, personal artist edit or hiding correction, a current observation, and no earlier name check for the same observation and personal artist operation. Fewer than two tracks can never corroborate, so their name is never sent. Credits are read in identifier order, at most eight per request.

How the name is compared. A name is normalized by collapsing runs of whitespace to one space, trimming, and lower-casing (Unicode lower case). Punctuation and diacritics stay significant ("Beyoncé" is not "Beyonce"; "Nova-Quartet" is not "Nova Quartet"). The same normalization compares track titles.

Steps and bounds.

1. `GET /ws/2/artist?query="<credit name>"&limit=100` (`artist_name_search` in the MusicBrainz client). Only artists whose normalized name equals the credit's count, minus the special-purpose artists. Each result carries MusicBrainz's relevance `score`, and an exact phrase match scores 100. The search is *conclusive* when the page holds every score-100 result: the whole result set fits on the page, or the last result on the page scores below 100 (scores must not rise down the page; a response that breaks that is invalid). A page that ends at score 100 with more results behind it may hide an exact-name artist, so the credit is uncertain. Many partial-name results ("Ed Sheeran & Justin Bieber", tributes) therefore no longer make a search inconclusive, which they did under the first version of this rule (any result count above the 10-result page was uncertain). More than three exact-name artists is also uncertain. Search answers are cached within the request.
2. For each exact-name artist, `GET /ws/2/recording?artist=<id>&limit=100&offset=<n>`: titles and lengths of recordings, and MusicBrainz's total. Pages are read in order until the library corroborates the artist (two distinct titles), the list is complete, or the artist's page allowance is used. One credit may read at most six recordings pages in total and at most five for one artist, split evenly between namesakes (three namesakes get two pages each), so a credit costs at most seven requests inside the eight-request batch cap. A prolific artist whose matching titles sit beyond the first hundred recordings is found by paging. Release-group browse plus a track list per release was rejected: it needs one request per release and release groups carry no lengths. An artist whose list was not completed and that did not corroborate is *not fully read*.
3. A library track corroborates when a listed recording has the same normalized title and both lengths are known and differ by at most two seconds (2,000 ms, inclusive). A length below one second (including 0, which stands for an unknown duration) is unknown on either side and never matches. An artist is *corroborated* when the matched tracks cover at least two distinct normalized titles; several tracks titled alike (two "Intro" recordings) count once.
4. The credit is accepted only when exactly one exact-name artist is corroborated and no other exact-name artist was left not fully read without corroborating. Two corroborated artists, or one corroborated artist beside a namesake that cannot be excluded, stay uncertain. Release-group titles alone are not used because they carry no length; the rule "two recordings, or one release with at least three tracks" is therefore implemented as the stricter two-recording rule.
5. The library tracks that individually matched receive the artist (provenance `matched_name_discography`, through the shared apply step, so all personal-decision rules hold). The credit-level step below then extends the artist to the credit's other undecided tracks that have no evidence of their own. Tracks that remain uncertain are remembered in `artist_name_checks` until their observation changes.

Rule version. The first version of this rule treated any search with more results than one page as uncertain, which left prolific credits such as a 106-track "Ed Sheeran" unmatched. Migration 0071 deletes every `artist_name_checks` row, which were all decided under that rule, and queues a walk (or sets the rerun flag of a walk under way) for each account that had any, so the next automatic walk examines those credits again.

A request makes at most eight MusicBrainz requests, checks the nine-second window and three-consecutive-failure limit before each one, abandons a lookup running 13 seconds after the pass started, and stops before a later credit that could exceed the eight-request cap (a credit needs at most seven: one search and six recordings pages). When the budget closes during the first credit of a batch its lookups count as unavailable and are retried by a later pass; otherwise the remaining credits wait for the next cursor. Requests share the one-per-second MusicBrainz gate and the admission slots. A closed page cancels the lookup in flight and nothing is written. A 404 from the search is an empty answer; outages, timeouts and invalid answers are *unavailable*: counted, not remembered, retried by a later pass. The receipt counts per recording, so a recording that the evidence phase counted as uncertain is counted again when the name phase also finds it uncertain; the page therefore labels the figure "uncertain answers" rather than distinct recordings.

What is sent. The credit name only, in the search query, and the MusicBrainz artist identifier in the recordings browse, with the Hito user agent. Titles and lengths are compared locally. Names, titles, queries and identifiers are never logged; the workflow logs counts, outcomes and timings. A deployment that must not send names turns metadata enrichment off for the account (the pass then does nothing) or never selects the built-in provider.

Limits. Artists with more than five pages (500 recordings) are partly read when their matching titles come late. A partly read artist that corroborates is accepted only when every other exact-name artist was fully read without corroborating; a partly read namesake that does not corroborate keeps the credit uncertain. A search whose page ends at score 100 with more results is never conclusive. Equal-name artists without enough library overlap are never matched.

### Credit-level matching

One identified track identifies its credit. After the individual matches of a settlement are planned (tag, ISRC, artist page or name discography), `credit.rs` extends each credit that has a clear majority artist: the artist named by more of the credit's individually matched tracks than all others together. The credit's other undecided tracks that carry no evidence of their own then receive that artist in the same immediate transaction, through the same apply step as every other match (so a personal choice, reset, split, unlink, personal artist edit or hiding correction still leaves a track alone). Rules:

- A track with its own evidence (an ISRC, an artist page or a file tag artist identifier on its current observation) is never followed. Its own lookup decides it, so a sibling whose ISRC names another artist, or several, is not overridden by the credit and stays uncertain or is matched to its own artist.
- Credits without a clear majority (two tracks naming two different artists) extend nothing.
- Hito does not parse credits. A credit name that lists several artists (`&`, `,`, `;`, `/`, `+`, `feat`, `ft.`, ` and `, ` with `, ` x `) is never extended, because one artist cannot stand for the whole credit.
- Two credits that merely share a name are separate credits with separate evidence. Nothing is extended across credits; a name alone is still never evidence.
- At most 1,000 siblings follow in one settlement, in recording order. The rest follow when a later settlement of the same credit runs, or are matched by the name phase.
- Provenance is `artist_association_credit_matches` (migration 0071): the originating evidence kind and the number of the credit's tracks that supplied it. The association API returns evidence `matched_credit_evidence` with `credit_evidence { kind, tracks }`, and saved artist reviews say, for example, "Matched automatically from 3 tracks' ISRC in this credit." Rows are immutable and are retained or erased with the other provenance tables.

## Release and recording identifiers from tags

File tags can also name the release, release group, recording and release track a file belongs to. Since inspection version 5 these are kept as raw assertions, and each well-formed one becomes an immutable evidence row (`recording_musicbrainz_observations`, migration 0064) for that recording and release. See [the tag details](metadata.md#release-and-recording-musicbrainz-identifiers-in-verified-source).

```mermaid
flowchart LR
    Tag[File tags: release, group, recording, release track ids] --> Inspect[Inspection version 5: raw assertions]
    Inspect --> Valid{Well-formed UUID?}
    Valid -->|no| Raw[Raw tag only]
    Valid -->|yes| Row[Immutable evidence row for the recording and release]
    Row -.->|not yet| Later[Lookup, provider links and display]
```

Nothing is merged or applied from these rows: releases with the same identifier stay separate, no recording is grouped by them, and personal edits are untouched. They are the verified starting point for later release and recording matching.

## Automatic matching and consent

Product direction: the app does this by itself. A listener who has not turned it off never presses a button. The tag pass reads local files only. The provider pass sends ISRC codes, provider artist links and, in its name phase, credit names to MusicBrainz, so the account's consent is a saved setting: **Look up artists on MusicBrainz automatically** (Settings, Privacy; `automatic_artist_lookup`, default on, stored per account in `account_metadata_automation`, changed through `PUT /api/v1/metadata/automation` and read in `GET /api/v1/metadata/settings`). It applies only while metadata enrichment is enabled and the built-in MusicBrainz provider is selected. Off means no automatic job and no automatic lookup. The manual walk (`POST /api/v1/artist-matching` and `/provider-evidence`) is unchanged and still works, because the listener asks for it explicitly.

```mermaid
flowchart LR
    Scan[Folder scan completes] --> Request
    Import[Playlist import or follow refresh] --> Request
    Request[Upsert one job row per account] --> Jobs[(artist_matching_jobs)]
    Jobs --> Worker[Matching worker, one step per second]
    Worker --> Tags[Tag pass]
    Tags --> Evidence[Provider evidence pass]
    Evidence --> Names[Name pass]
    Names --> Genres[Genre pass]
    Genres --> Done[Delete job, record result]
    Worker -.->|settings off| Drop[Drop job, no lookup]
    Done --> Covers[Queue covers for albums without local files]
```

How the job works (current behavior, covered by owned fakes):

- A trigger is an upsert of the account's row in `artist_matching_jobs` inside the transaction that adds the recordings: scan completion (every account with access to the scanned library), playlist import and follow create/refresh. The row is skipped when the account's settings forbid automatic matching. Repeated triggers share one row (single flight per account). A trigger that arrives while the walk is under way sets a rerun flag, so one more walk follows. When the server starts, the matching worker also queues one first walk for each account that can read a library, allows automatic matching, has no job row and has never finished a walk, so a library that predates the worker is not left at "No run yet". Accounts with a job or a finished walk are left alone, so a restart never repeats work or sets a rerun flag.
- The server runs one worker. Each second it claims the oldest due row with a 90 second lease, runs exactly one batch (the tag pass, then the evidence and name passes through the same cursor the web app used), stores the phase and cursor, and releases the row. The batch uses the same bounds, admission slots, per-recording remembered checks and the one request per second MusicBrainz limiter as a manual run. The cursor is in SQLite, so a restart resumes and an expired lease is simply claimed again.
- A shutdown drops the step in flight (cancelling any lookup), clears the lease and keeps the position. A busy admission slot retries in two seconds. An internal failure backs off (one to sixty minutes, six attempts) and then abandons the row. A walk whose lookups were unavailable is repeated later with exponential backoff, up to six times.
- The worker acts as the account without a browser session (an unattended actor limited to core). It still goes through the same policy read, so a disabled provider, a removed account or the setting turned off drops the row without any request.
- `GET /api/v1/artist-matching/status` reports `enabled`, `state` (`idle`, `queued` or `running`), `matched_so_far`, `last_finished_at` and `last_matched`, so the Artists page can show a status line instead of running its own pass.

**On by default after an upgrade.** Accounts that never saved the choice, including every existing account, have it on. Once the walk reaches the provider passes, MusicBrainz receives the ISRC codes of tracks that have one, the artist page links of imported tracks, and, for tracks it cannot place otherwise, the credit names of those tracks (never titles of albums, paths, account names or audio). Turning the choice off in Settings, Privacy stops all of this; the Privacy description says so. The background walk uses its own admission slots, separate from the ones an account's profile, preview and review requests use, so it never makes those requests busy.

Logs carry the phase, a safe outcome and elapsed time only; never an account, a name, a title, an identifier or a search term.

Applying. Recordings are re-validated inside one immediate transaction exactly like the tag pass: the current observation must still be the one examined, a personal choice, reset, split, unlink, personal artist edit or hiding shared correction leaves it alone (and is never looked up in the first place), and a stale automatic match whose observation changed may be matched again. A match writes a reviewed association for the recording's own credit and the chosen artist; a new association gets a row in `artist_association_provider_matches` with `isrc` or `provider_url`, returned by the association API as `matched_isrc` or `matched_provider_url`. Exact retries replay the stored receipt without another lookup, only a request that matched something stores a receipt, and the receipt counts `matched`, `uncertain`, `declined` and `unavailable`.

The reported example follows this path: the local C4 Pedro file carries one MusicBrainz artist id, so the tag pass groups it. An imported Spotify-style track with an ISRC that MusicBrainz maps to a recording credited only to that artist is matched by the provider pass and joins the same group without a click. The current public Spotify playlist response states no ISRC, so a real Spotify import supplies this evidence only through its credited artist page; see [the Spotify plugin](plugin-spotify.md#artist-pages-and-isrc).

## MusicBrainz genres

The last pass of the automatic walk reads the official MusicBrainz genres of the identities the account's library names, so that Discover "By genre" is not empty for imported music (current behavior, covered by owned fakes only; the live service has not answered these requests). It sends only MusicBrainz identifiers, under the same consent (`automatic_artist_lookup`), the same one request per second limiter and the same background admission slots as the other passes.

- Candidates, in this order: the MusicBrainz artists of the account's chosen artist groups (special-purpose artists such as Various Artists are skipped), then the release groups and recordings named by retained file tag identities of recordings that have no file `GENRE` tag. Each is looked up with `artist/<id>`, `release-group/<id>` or `recording/<id>` and `inc=genres`; only the `genres` list is read, never `tags`.
- Batches are bounded like the other passes: at most eight lookups, a nine second window, a thirteen second deadline and a stop after three consecutive failures. The position is stored in the job cursor as `genres:<rank><identifier>`, so a restart or shutdown resumes there. A shutdown drops the lookup in flight and writes nothing.
- Answers are cached by identifier in `musicbrainz_genre_lookups` and `musicbrainz_genres` (migration 0069; at most eight genres per identity, names normalized as for file tags). A cached answer, including an empty one, is not asked again for 30 days. An unreadable answer is cached as empty and asked again after seven days. An outage caches nothing, marks the step unavailable and repeats the walk later with the existing backoff; a stale answer is replaced when it is asked again.
- Starting the server queues a walk for an account that finished earlier walks when any of its identities has no lookup younger than 30 days, which is how a library that predates the pass gets its genres.
- The cache is public reference data without an account column. Account erasure removes the lookups no remaining account's groups or retained tag identities name.
- Logs carry one event per step with the outcome, the number of lookups, the number unavailable and the elapsed time. No identifier, genre name, title or credit appears. The client logs a static outcome, a count and the elapsed time per request.

The genre pass does not change what a match is: `matching_matched` counts only artist matches.

## Suggestions for uncertain cases

`GET /api/v1/artists/{id}/match-suggestions` lists chosen artists whose members carry this credit's name (compared without case or spacing). Each item states what supports it:

- `shared_albums`: distinct release titles of the credit's undecided tracks that also appear on the chosen artist's tracks.
- `same_length_tracks`: undecided tracks with a chosen track of the same title (ASCII case folding) whose length differs by at most three seconds.
- `undecided_tracks`: tracks of the credit with no current choice that accepting would update.

Items rank by `10 × shared_albums + same_length_tracks`, then group identifier. A chosen artist with no undecided track is not listed. A name-only item is shown as "same name only" because the manual **Choose the artist** list already offers it. Candidate groups come from one query on member credit names (`lower(trim(name))`, so only ASCII case folding and no internal-space collapsing; a missed Unicode namesake is only a missed suggestion). The credit's undecided tracks are read once, and group summaries are read only for suggestions that survive. The same read also returns `chosen` (the chosen artists the credit's tracks already use, with how many of those tracks Hito matched itself) and `undecided_tracks`, which the credit page shows. `undecided_tracks` on an item excludes tracks the apply would keep (a different choice, or an unlinked review of the same artist). Reading suggestions writes nothing and contacts no provider; it is bounded to 50 groups, 500 credit tracks and 500 tracks per group, inside the standard 15-second budget, and a work-budget test measures 20 namesake groups at about 126,000 SQLite steps (the first version used about 206,000). Exceeding the budget answers a conflict.

Accepting uses the existing confirmation and `POST /api/v1/artists/{id}/artist-identity` batches, so all the rules of [applying one identity to a credit](artist-identity-groups.md#applying-one-identity-to-a-whole-credit) apply: a different existing choice is kept, an unlinked association is respected. `POST /api/v1/artists/{id}/match-suggestions/dismiss` remembers "not the same artist" for this credit and chosen artist, per account, and returns the refreshed list.

Restoring. The read also returns `dismissed_items`: the dismissed suggestions that could still apply to undecided tracks, each with the same evidence as a listed suggestion (`dismissed` is their count). `POST /api/v1/artists/{id}/match-suggestions/restore` with the same `{group_id}` body deletes that one dismissal row for this account, credit and chosen artist and returns the refreshed list, so the suggestion is offered again. Restoring is idempotent: restoring a suggestion that was not dismissed, or restoring twice, changes nothing. A restore never applies a choice. It does not use a request receipt or revision, like the dismissal it undoes, because it deletes one row that only hides a suggestion. An unknown credit or chosen artist, or one of another account, is a 404; a locked session is a 423. The credit page shows a collapsed "Dismissed suggestions" disclosure with a **Restore** button per row, also when every suggestion is dismissed.

## Web behavior

The server runs the matching walk, so the Artists page starts no pass when it opens and keeps no timer in local storage. It reads `GET /api/v1/artist-matching/status` and shows one muted line:

- while a walk is `queued` or `running`: "Matching artists… N matched" (the walk's `matched_so_far`);
- when idle: "N matched · last run 5 minutes ago" (`last_matched` and `last_finished_at`), or "No run yet." before the first walk finishes;
- nothing at all when the account's settings do not allow automatic matching, or when the status cannot be read.

The line also says that artists are looked up on MusicBrainz and links to Settings, Privacy, where the choice lives (see [automatic lookups](metadata-preferences.md#automatic-lookups)). The status is read again every three seconds while a walk is queued or running and every fifteen seconds otherwise. Polling pauses while the tab is hidden, reads once when it is shown again, stops when the page is left and does not run while matching is off. When a walk ends, the artist lists are refreshed once.

**Match again** (shown only while no walk is active) runs the manual provider pass: it walks the cursor of `POST /api/v1/artist-matching/provider-evidence` (at most 200 batches or two minutes), from the evidence phase into the `name:` phase, stops when a batch reports an unavailable provider or a cursor does not advance, refreshes the lists once if something matched, reads the status again and is cancelled when the page is left. It never starts by itself. The browser no longer calls the tag pass `POST /api/v1/artist-matching`. The saved artist reviews of a recording say how an automatic match was made (file tag identifier, ISRC, provider artist page or name corroborated by recordings); reviews the account confirmed itself carry no such note. The credit page no longer asks the listener to find reviewed artists. Its Artist identity section shows one of three states from the suggestions read and the matching status: "Artist: <name> · matched automatically" (or "chosen by you", or "N of M tracks matched automatically") with **Open artist**; "Hito is identifying this artist…" while a walk is queued or running (the page reads the suggestions again when the walk ends); or "Not identified yet" with a secondary **Choose the artist** button. The button opens a panel with the earlier manual flow (suggestions, the reviewed-artist list, confirmation and the batch apply), the same principle as the Change artist panel on Recording Details. A credit that is matched but still has undecided tracks keeps the button for those tracks. A failed read shows nothing and leaves the manual flow untouched. Neither workflow subscribes to playback position.

## Logging

Every request logs an operation, a safe outcome, counts and elapsed time. Identifiers, credit names, titles, paths, tag values, ISRCs, artist page addresses, MusicBrainz identifiers and response bodies are never logged. The MusicBrainz client logs one completion event per lookup with a static outcome, the item count and its elapsed time.

## Verification

Public core cases use real temporary SQLite and owned deterministic WAV files tagged through lofty:

- Two credits sharing one embedded identifier group, a namesake with another identifier stays separate, and an untagged equal-name file, a two-identifier file and an imported playlist track are untouched. Recordings, sources, details and activity are byte-for-byte unchanged.
- Exact replay, conflicting reuse, a second pass that changes nothing, and resumable one-recording batches.
- A personal reset and a personal unlink are not overridden, an unlink made before any choice is respected, and disabled enrichment matches nothing.
- Suggestions rank by evidence, include name-only items last, never change a choice, can be dismissed per credit and chosen artist, and end once accepted through the existing path.
- Another account's pass cannot undo an administrator's shared correction; a retagged automatic match is matched again while a reset or split one is not; personal artist edits, splits and retired choices are left alone; two concurrent passes match each recording once; a second pass reads no candidates and writes nothing; and work-budget cases bound the pass and the suggestions.
- Release and recording identifiers: five tag dialects (the matrix), an identity case with real temporary SQLite (well-formed, malformed, upper-case, shared across two releases, no merge, a second scan adds nothing, a personal title edit survives) and migration 0064 with exact-schema upgrade coverage and immutability.
- Migration 0060 upgrades from 0059 with exact schema comparison, immutable provenance, a per-account dismissal key and no implied matches.

Provider evidence and name corroboration are covered at three boundaries:

- Protocol: the MusicBrainz client against an owned loopback server for exact request targets and headers, 404 as empty, 503 as a typed failure without retry, wrong or malformed identifiers, over-long answers and log redaction; the plugin SDK for ISRC normalization and canonical artist page validation with old payloads keeping their exact bytes; the folder and Spotify plugins for what they publish; and FFmpeg-backed inspection of ISRC tags in FLAC, Ogg, MP3, M4A and WAV.
- Core, with real temporary SQLite, deterministic WAV tags and an owned loopback MusicBrainz: a tagged local track and an imported track with an ISRC (written with hyphens and lower case) join one group without a click; ambiguous, multi-artist, unknown, Various Artists and outage answers do not; a file-tag ISRC matches; a lone artist page matches and falls through from an unknown ISRC; two pages are never looked up; a reset recording is never looked up or overridden; disabled enrichment asks nothing; outages are retried while settled answers are not asked again; exact replay does not ask again; a cursor walk examines every recording; and a pass dropped while the fake is slow writes nothing and can be repeated. Migrations 0061 and 0062 upgrade with exact schema comparison, immutable rows and no implied evidence.
- Name corroboration: the MusicBrainz client's recordings browse against the owned loopback server (request target, lengths, incomplete totals, 404 as empty, 503 without retry, invalid answers, log redaction); thirteen core cases through the public capability with real temporary SQLite and the owned fake (a unique corroborated match, two corroborated namesakes, a non-corroborating namesake, a namesake not fully read, below-threshold and length-tolerance edges, an unknown name and settled answers not asked again, outage retried later, cancellation writing nothing, disabled enrichment and the silent tag pass sending no name, personal reset left alone and a one-track credit sending nothing, exact replay, and the eight-request cap with a resumable cursor); unit cases for name normalization, the truncated-search rule, the per-lookup window and the failure limit; migration 0063 with exact schema comparison.
- Score rule, paging and credit-level matching: an Ed Sheeran-like search (exact artist plus 99 partial-name results, total 4,000) is conclusive and matches; a page of 100 score-100 results with more behind it is not; recordings found on the third page; a 700-recording artist stops after five pages; one ISRC-matched track plus 20 siblings matches all 21 with `matched_credit_evidence` and a single MusicBrainz request; siblings with their own conflicting evidence are decided by it; no majority extends nothing; a reset sibling is left alone; a credit listing several artists extends nothing; the credit read reports the chosen artist; migration 0071 expires name checks and queues walks. The assertion-level failing logs of the first versions were recorded in the session scratchpad (`red-name-fix-search.log`, `red-name-fix-credit.log`).
- HTTP and web: the reported scenario through the real router (statuses, validation, replay, conflict, 401 and 423 while locked, the `matched_isrc` association), and DOM cases for the explicit provider action (no provider request on page load, a click walks the cursor and shows the counts, one refresh, no retry on an unavailable provider, disabled when MusicBrainz is off, cancelled on leaving) and the provenance text. Further core cases cover an address outside the allowlist (never looked up, even when stored by an earlier version) and the per-lookup time window and deadline.

Not verified: MusicBrainz itself. Every response in these tests is an owned fake that follows the documented JSON shapes; a live `isrc`, `url`, artist search or recordings browse has not succeeded (the 2026-10-18 live attempts for the existing adapter received HTTP 503, see [the adapter](builtin-musicbrainz.md#live-integration-gate)), and the live gate does not yet cover these lookups. The Spotify artist page field relies on the `uri` of each artist in the undocumented public response, which the plugin's owned fixtures include but a live import has not confirmed. Work budgets for the candidate query were not measured (the tag pass has a measured budget).

HTTP cases use the real router for statuses, validation, replay, access, a second account without library access, two members of one shared library (separate matches, choices and dismissals) and 423 for all three routes while the session is locked. Web cases cover evidence text, confirmation before apply, dismissal, the dismissed-suggestions disclosure and restore, failed reads, and the status line (idle, queued and running text, polling every few seconds, no polling while hidden, after leaving or while disabled, one refresh when a walk ends, no pass started on open, and the manual Match again walk with its cancellation).

## Open work

- Automatic matching is covered by owned fakes only; an earlier manual live run is recorded as matching sixteen recordings, but the adapter's live gate has only seen HTTP 503, so live behavior stays unverified. The automatic job runs on the local development server and has not been measured against the live service. A per-account job runs one batch per second at most, so a very large library takes a long time by design.
- Account deletion erases the account's `account_metadata_automation`, `artist_matching_jobs` and `cover_art_lookups` rows with its other private rows ([account deletion](architecture.md#account-deletion)).
- A persistent cache of MusicBrainz artist answers beyond genres, shared across accounts (genres are cached, see above), re-checking of "unknown" answers when MusicBrainz gains data, and a work-budget measurement of the provider candidate query on a large library.
- Artist page evidence for existing Spotify imports. A followed playlist stores the pages on its next refresh (the follow refresh re-observes every recording, and the changed digest adds an observation that carries the pages). A one-shot imported copy only receives them if the playlist is imported again: the existing **Find missing albums** repair covers recordings without a release only, and the plugin offers no per-track lookup. No new Spotify API call was added. Credit-level matching reduces the need, because one ISRC, page or corroborated title identifies every sibling without evidence.
- Providers that expose ISRCs or artist pages. The folder plugin publishes ISRC tags. The public Spotify response states no ISRC. No other bundled plugin has an ISRC field in its source API that Hito could verify; none was invented. Existing imported copies receive artist pages only when their metadata is refreshed from the provider.
- Name matching is implemented against owned fakes only. Still open: live acceptance, a persistent cache of search and recordings answers shared across accounts, release-based corroboration (release groups carry no track lengths), reading beyond five pages of an artist's recordings, and a work-budget measurement of the name candidate query on a large library. 
- Genre lookups have not run against the live service, and a library with thousands of tagged recordings makes one recording lookup per recording at one request per second, which can take hours; recording-level genres for files that carry a recording identifier but no genre tag are the slowest part.
- Release and recording matching beyond retention. Local files now retain release and recording MusicBrainz identifiers as evidence (see below), but nothing yet looks them up at MusicBrainz, links a release or recording to a provider entity, shows the identifiers on album or recording pages, or reads an ID3 `UFID` recording identifier.
- Live MusicBrainz acceptance remains blocked by upstream HTTP 503 responses; see [the adapter](builtin-musicbrainz.md#live-integration-gate).
