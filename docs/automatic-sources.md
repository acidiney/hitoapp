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

Each attempt considers at most four approved personal connections, twenty search results per connection and three detailed candidates per connection. Exchanges are sequential within an attempt.

Caller admission permits one in-flight resolution call per account and two globally across cloned catalog handles. Excess callers receive a controlled conflict before entering SQLite admission; they do not wait or create observer aliases. Durable pending resolutions retain their account/global limits.

A ten-second budget bounds admission and read-only preparation; a twelve-second lease and token checks fence publication. A started SQLite publication is awaited and can extend beyond the foreground budget. Caller capacity remains held through publication and the final authorized result.

This policy does not yet handle common YouTube video-title wrappers. A title such as `Performer - Song (Official Audio)` can be rejected before detailed extraction. The SDK and YouTube adapter now have a separate [musical-metadata endpoint](plugins.md#musical-metadata-in-development) that retains track title, album context and source witnesses alongside the full video title. Core does not yet request that evidence. Missing album context can therefore still conceal a live-version qualifier during automatic selection. Provider metadata is not acoustic verification; these gaps must be resolved before real automatic YouTube acceptance.

## Retry and cancellation

Core stores durable request receipts. Replaying an accepted UUID returns the same resolution identity and rechecks access before returning current sources. A historical `ready` outcome is not current playback authority. Reusing a request UUID for another recording conflicts.

The browser freezes one UUID and request body. An ambiguous acknowledgement permits one retry with that exact body and the same account-owned cancellation signal. Rejected client requests are not retried automatically. Account or queue replacement cancels the old owner and fences late results. A resolution response cannot replay an audio source already exhausted in that playback cycle.

Supplied cancellation can interrupt read-only candidate work. Once publication starts, core awaits its fenced transaction before settling the caller. Abruptly dropping the entire core future and HTTP disconnect ownership remain separate verification work. No detached cleanup task is used to claim immediate release.

## Verified evidence

- The first public HTTP case passes identity preservation, candidate filtering, immutable receipt replay and current source privacy against a real temporary server, SQLite and owned protocol provider.
- The populated migration-35-to-36 case passes exact preservation of prior typed rows, rowids, migration checksums, sequences, provider binding history, private state, downloads, artwork and preparation references, with foreign-key, integrity and FTS checks. It is an owned fixture, not a stopped whole-volume rehearsal of the normal installation.
- Migration regressions populate 259 and 4,099 bindings with recording, source, artwork, preparation and download references. Actual migration work stays below the unchanged budgets with absent, inherited and fresh planner statistics: approximately 146 to 150 thousand and 1.949 to 1.953 million SQLite VM steps respectively. The original 100,000-recording upgrade also passes, with a 10.3-second reopen in its development smoke test. These are migration checks, not release latency benchmarks.
- The publication-cancellation regression first failed after a held SQLite writer was released. With the fix, the same case passes accepted association settlement, receipt replay, unchanged primary metadata and a subsequent Play without another provider search. The migration and HTTP cases and strict production Clippy pass with that fix.
- Two React-owner cases pass account-switch cancellation and exact-body retry after a lost acknowledgement. They preserve duplicate queue occurrences and use the real browser player with fetch and media boundaries. They do not prove real browser decoding or server restart durability.
- The complete frontend suite passes 220 cases across 61 files, both TypeScript checks and the production build.
- A held provider-source exchange followed by public connection removal now stops later candidate requests. The regression preserves primary metadata, personal edits, activity and duplicate playlist entries. Its active control still reaches the next eligible candidate.
- Public matching cases reject studio-to-live and live-to-studio associations when the release is titled exactly `Live`. A studio song whose musical title is `Live` still resolves and plays its exact owned WAV.
- When candidate A already belongs to another Hito recording, resolution skips it and selects B. Both recordings retain their identities and metadata; A still serves its original WAV and the target plays B's distinct WAV. A binding claimed after candidate selection remains separate unfinished work.
- Six excess same-account calls return controlled Busy while an owner is held. After settlement, its UUID replays and rejected new UUIDs receive distinct fresh receipts. A separate three-account case admits two held owners, rejects the third without provider IO, then admits that account when one owner settles. Its receipts and source access remain private to each account. These cases and their strict Clippy checks pass; whole-future-drop capacity and durable-lease recovery remain separate boundaries.
- The SDK passes seventeen public contract cases with schema generation enabled. YouTube passes nineteen fixture cases, with one live case explicitly ignored, and both pass strict all-target Clippy. New cases cover structured artists, retained video-version titles, malformed lists, legacy fallback and a separate snapshot-bound musical-metadata endpoint. These are owned extractor/HTTP fixtures, not live automatic matching.

Release image `sha256:5c9f204c7fb22705223bc7ca37c69360f50fcf4b5ec67f4b26a7b12ddcc766bf`, built from `47100cb`, passes the isolated Chromium workflow. It imports duplicate unavailable entries, preserves personal edits and likes, presses ordinary Play, rejects incompatible candidates, selects the studio source and advances an owned WAV through Hito's relay. It creates no second recording, manual attachment or persistent download. Health, exact OpenAPI and served JS/CSS, anonymous native-player rejection and fixture log-redaction checks pass on that same image. Its temporary server is stopped.

That image precedes the publication-cancellation fix. A final accepted image must include the fix and pass runtime verification again. Neither this candidate nor the separately verified preparation UI has replaced the normal server.

## Remaining work

Durable manual selection must survive automatic retries, Follow updates and restart. Choosing provider binding B must exclude known-wrong binding A and its saved copies while allowing a verified saved copy of B during an outage. Download admission must not reuse A solely because it belongs to the same recording. Explicit attachment remains an independent operation that adds an alternate source.

The first independent security, correctness and performance review found five issues. Directed regressions and fixes now cover migration child-reference work, candidate calls after revocation, an explicit release titled Live and bounded waiting-call admission. The already-bound candidate case passes when the binding exists before selection; a claim arising at publication still requires continuation without restarting search or increasing the original budgets. A second review round has not started. The fresh complete core/server gate stopped at the original large migration regression and has not been rerun to completion; directed checks above remain separate evidence.

Other required work includes retained automatic-decision revalidation after metadata changes, remaining authority and concurrency boundaries, failed-ready-source recovery, core consumption and retention of musical metadata, real Spotify/YouTube playback, native integration, final review, full upgrade rehearsal and platform, codec and performance gates. The automatic-source item in [TODO.md](../TODO.md) remains open.
