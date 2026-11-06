# Choosing the audio source for a recording

A recording can have several playable audio sources: a file in a music folder, a copy Hito saved, a podcast enclosure, and one or more sources from connected providers. This page records which one plays first, and what happens when it does not open.

## Current behavior: the order today

The order is the same for every account except where the account's own data differs. Nothing is user-set yet.

1. **Manual override.** A source the listener chose for this recording (see [manual sources](manual-sources.md)) makes every other provider source of that recording not ready for that account. Local files and saved copies stay available. A choice is never changed by ranking or by a failed open.
2. **Local files.** A file in a connected music folder that is available on this host.
3. **Saved copies.** A managed download that is ready and whose origin is permitted by the current choice.
4. **Podcast enclosure**, for podcast episodes.
5. **Provider sources**, in the order the community ranking gives them when the listener opted in, otherwise by source ID. The community ranking only applies when the recording has no ready local file or saved copy, at least two ready provider sources exist, and the recording has no protected choice. It reorders the ready provider sources of that one recording, including across providers. If the ranking service is slow, unreachable or inconsistent, the unranked order is used.

Where each surface applies it:

| Surface | Where the order comes from | If a source fails |
| --- | --- | --- |
| Browser | `GET /api/v1/recordings/{id}/sources` (`Catalog::playback_sources_until`) returns the sources in the order above. The player takes the ready ones in that order. | `PlaybackSources` moves to the next ready source, once per source, with no limit on how many. When none is left it refreshes the provider sources once, then asks Hito to find audio once, then skips the track. |
| Desktop (native) | `PlaybackAccess::playback_candidates` yields local files, then saved copies, then the podcast enclosure, then each provider binding in binding order. The community ranking is not applied. | The player asks for the next candidate until they run out, then refreshes once, then reports the error. |
| Single source | `PlaybackAccess::preferred_playback_source` (local, then saved, then podcast, then provider). Used for one-shot reads. | Not applicable. |

Every candidate is authorized for the requesting account when it is read and again when it opens. Another account's sources, connections and choices are never in the list.

What does not exist today: a way to rank providers against each other, a way to turn automatic fallback off, a bound on fallback attempts, and a record of why a source was skipped.
