# Reviewed artist discography

Core and an authenticated HTTP route can read one release-group page for an explicitly confirmed recording-credit association. The provider and provider artist identity come from that retained association. Callers supply a page limit and optional Hito continuation, rather than a separate provider identity. Shared browsing controls and normal-server publication remain unfinished.

## Ownership

```mermaid
flowchart LR
    Caller[Recording and exact association] --> Authority[Capture current authority in core]
    Authority --> Token[Validate Hito continuation]
    Token --> Provider[Negotiate artist discography]
    Provider --> Page[Read one bounded SDK page]
    Page --> Recheck[Recheck authority in core]
    Recheck --> Result[Page and protected continuation]
```

`Catalog::recording_artist_discography_until` owns current-session and recording access checks, retained association identity, metadata policy, provider connection and the returned page. SQLite transactions finish before provider HTTP. Reads do not associate artists, adopt releases or recordings, merge namesakes, append collection events, prepare sources or open audio.

The built-in MusicBrainz adapter and approved personal HTTP catalogs use the shared SDK contract. An HTTP provider must independently declare `ArtistDiscography`; profile support alone is insufficient. Release-group identities, ordered credits and unknown snapshots/revisions remain unchanged. A release group does not select an edition or supply its tracks. Same-title groups remain separate.

## Continuation and bounds

The app-owned request accepts a limit from one to 50 and an optional ASCII continuation of at most 8,192 bytes. Core authenticates the continuation with a process-owned XChaCha20-Poly1305 key and domain-specific associated data. The envelope retains provider cursor, snapshot and limit plus a bounded fingerprint of the originating session, account, full current credit, exact association, policy revision and connection/service revisions. Tampering fails validation; a changed authority or limit requires starting again. Both fail before provider negotiation or page requests.

The key is shared by cloned capability handles. Restarting the process invalidates previous continuations. Logging out and signing in again also requires starting pagination again, even for the same account. This is a pagination lifecycle rule; the continuation never replaces fresh authorization.

Serialization, Base64 decoding and authority hashing have fixed bounds. HTTP plugin pages have a 256 KiB wire limit; SDK output has at most 50 items and 256 KiB. The built-in adapter separately caps its raw MusicBrainz browse response at 1 MiB before projection. A call reads one page and never crawls later pages automatically. MusicBrainz browse is live, with no invented immutable snapshot.

The existing metadata admission owner permits four workflows globally and one per account, independently of playback-source capacity. One 15-second deadline covers local authority checks, negotiation and the page request. Cancellation drops the inline request future. Workflow logs contain generated correlation IDs, safe outcomes, item counts and elapsed times, without cursors, session material, names or provider addresses.

## Verification and remaining work

`POST /api/v1/recordings/{id}/artist-associations/{association}/discography` uses the shared account and JSON extractors, the core cancellation boundary and private, no-store responses. It accepts `ReadArtistDiscography` and returns `ArtistDiscographyAvailability`. The route performs no SQL or detached provider work. OpenAPI and TypeScript are generated from the actual Rust route and wire types.

Two public core cases use temporary SQLite and an owned HTTP catalog. They verify explicit confirmation, profile-only rejection, exact two-page SDK data, Unicode identities, unknowns, limit rejection, altered-token rejection and a changed policy revision rejected before another provider request. The first preserves original audio, private pins, duplicate playlist entries, likes, progress, sources, downloads and retained evidence. The second logs session A out, authenticates a new valid session B of the same account, and requires the old continuation to conflict without provider I/O.

The initial case first failed because the public capability and types were absent. The session case then exposed missing originating-session continuity; adding the existing session binding to the authenticated fingerprint resolves it. Both cases and 14 existing artist-profile/discovery cases pass together. Strict all-target core Clippy and formatting pass. Independent security, correctness and performance reviews pass, including a second correctness review of the session rule. After integration, both new cases, strict all-target workspace Clippy and formatting pass again.

The HTTP integration case first failed with a missing-route 404. It now passes with all 73 API-module cases, strict all-target server Clippy and formatting. It checks two pages, exact identities and unknowns, account isolation, origin and request guards, invalid limits and fields, altered continuations and changed authority. Private collection events, sources and downloads stay unchanged. An independent route review passes. After integration, the new case passes again with strict server Clippy, formatting and an identical Rust OpenAPI export. The generated change adds one route and six schemas; existing OpenAPI values remain unchanged, TypeScript generation reproduces and both frontend type configurations pass.

This is directed source qualification, not a complete release gate. Discography-specific held-request cancellation, deadlines, admission recovery, concurrent authority changes, maximum continuations and hostile response boundaries need further coverage. React integration, selected-edition traversal, local/playable availability, canonical identity corrections and deployment remain open. [Live MusicBrainz acceptance](builtin-musicbrainz.md#live-integration-gate) remains failed after HTTP 503 responses.
