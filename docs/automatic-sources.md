# Automatic audio sources

Automatic source resolution is in development on a separate branch. Its first controlled browser workflow passes, but the normal local server still runs the earlier image. Real Spotify-to-YouTube selection, durable manual correction and native integration remain unfinished.

## Playback and ownership

Ordinary browser Play can start a known recording even when the page's availability snapshot is false. The playback owner tries current sources, one refresh of existing bindings, then one automatic-resolution stage. Unknown retained playlist entries cannot start playback. Loading a page does not start resolution or a persistent download.

```mermaid
flowchart LR
  Play[Explicit Play of a saved recording] --> Player[Account-owned browser player]
  Player --> Existing[Current sources and one refresh]
  Existing -->|Exhausted| Resolve[Authenticated resolution API]
  Resolve --> Core[Core catalog]
  Core --> Evidence[Inherited musical metadata]
  Core --> Providers[Approved personal search and audio services]
  Providers --> Check[Check candidate details and readiness]
  Check --> Publish[Fenced SQLite publication and receipt]
  Publish --> Same[Same Hito recording with an alternate source]
  Same --> Player
```

Core searches with the recording's inherited metadata, preserving personal display edits. It does not give one plugin another plugin's recording identifier. An accepted alternate source keeps the original recording, primary metadata owner, playlist entries, likes and progress. Automatic associations carry their own policy evidence rather than pretending the user confirmed them.

The public command is `POST /api/v1/recordings/{recording}/sources/resolve` with a nonempty `request_id` UUID. The response contains Hito request, resolution and recording identities, a controlled outcome and current authorized sources. Rust handlers and wire types generate OpenAPI and TypeScript.

## Current policy and bounds

The first policy requires matching normalized titles and known musical artists. It rejects unknown or channel-only artist evidence. It compares visible live, remix, cover, karaoke, instrumental, slowed and sped-up qualifiers. Both durations must be known and at least one second. The permitted difference is two percent, bounded between three and ten seconds. These thresholds have deterministic fixture coverage; they have not been calibrated against a real music corpus.

Each attempt considers at most four approved personal connections, twenty search results per connection and three detailed candidates per connection. Exchanges are sequential within an attempt. Admission permits one pending resolution per account and two globally. A ten-second budget bounds admission and read-only preparation; a twelve-second lease and token checks fence publication. A started SQLite publication is awaited and can extend beyond the foreground budget.

This policy does not yet handle common YouTube video-title wrappers. A title such as `Performer - Song (Official Audio)` can be rejected before detailed extraction. The SDK and YouTube adapter now have a separate [musical-metadata endpoint](plugins.md#musical-metadata-in-development) that retains track title, album context and source witnesses alongside the full video title. Core does not yet request that evidence. Missing album context can therefore still conceal a live-version qualifier during automatic selection. Provider metadata is not acoustic verification; these gaps must be resolved before real automatic YouTube acceptance.

## Retry and cancellation

Core stores durable request receipts. Replaying an accepted UUID returns the same resolution identity and rechecks access before returning current sources. A historical `ready` outcome is not current playback authority. Reusing a request UUID for another recording conflicts.

The browser freezes one UUID and request body. An ambiguous acknowledgement permits one retry with that exact body and the same account-owned cancellation signal. Rejected client requests are not retried automatically. Account or queue replacement cancels the old owner and fences late results. A resolution response cannot replay an audio source already exhausted in that playback cycle.

Supplied cancellation can interrupt read-only candidate work. Once publication starts, core awaits its fenced transaction before settling the caller. Abruptly dropping the entire core future and HTTP disconnect ownership remain separate verification work. No detached cleanup task is used to claim immediate release.

## Verified evidence

- The first public HTTP case passes identity preservation, candidate filtering, immutable receipt replay and current source privacy against a real temporary server, SQLite and owned protocol provider.
- The populated migration-35-to-36 case passes exact preservation of prior typed rows, rowids, migration checksums, sequences, provider binding history, private state, downloads, artwork and preparation references, with foreign-key, integrity and FTS checks. It is an owned fixture, not a stopped whole-volume rehearsal of the normal installation.
- The publication-cancellation regression first failed after a held SQLite writer was released. With the fix, the same case passes accepted association settlement, receipt replay, unchanged primary metadata and a subsequent Play without another provider search. The migration and HTTP cases and strict production Clippy pass with that fix.
- Two React-owner cases pass account-switch cancellation and exact-body retry after a lost acknowledgement. They preserve duplicate queue occurrences and use the real browser player with fetch and media boundaries. They do not prove real browser decoding or server restart durability.
- The SDK passes seventeen public contract cases with schema generation enabled. YouTube passes nineteen fixture cases, with one live case explicitly ignored, and both pass strict all-target Clippy. New cases cover structured artists, retained video-version titles, malformed lists, legacy fallback and a separate snapshot-bound musical-metadata endpoint. These are owned extractor/HTTP fixtures, not live automatic matching.

Release image `sha256:5c9f204c7fb22705223bc7ca37c69360f50fcf4b5ec67f4b26a7b12ddcc766bf`, built from `47100cb`, passes the isolated Chromium workflow. It imports duplicate unavailable entries, preserves personal edits and likes, presses ordinary Play, rejects incompatible candidates, selects the studio source and advances an owned WAV through Hito's relay. It creates no second recording, manual attachment or persistent download. Health, exact OpenAPI and served JS/CSS, anonymous native-player rejection and fixture log-redaction checks pass on that same image. Its temporary server is stopped.

That image precedes the publication-cancellation fix. A final accepted image must include the fix and pass runtime verification again. Neither this candidate nor the separately verified preparation UI has replaced the normal server.

## Remaining work

Durable manual selection must survive automatic retries, Follow updates and restart. Choosing provider binding B must exclude known-wrong binding A and its saved copies while allowing a verified saved copy of B during an outage. Download admission must not reuse A solely because it belongs to the same recording. Explicit attachment remains an independent operation that adds an alternate source.

The first independent security, correctness and performance review found five issues requiring regression and resolution work: migration child-reference work, candidate calls after revocation, an explicit release titled Live, an already-bound candidate blocking later results and unbounded waiting-call admission. A second round has not started. The fresh complete core/server gate stopped at the large migration regression and has not passed; directed earlier checks remain separate evidence.

Other required work includes retained automatic-decision revalidation after metadata changes, remaining authority and concurrency boundaries, failed-ready-source recovery, core consumption and retention of musical metadata, real Spotify/YouTube playback, native integration, final review, full upgrade rehearsal and platform, codec and performance gates. The automatic-source item in [TODO.md](../TODO.md) remains open.
