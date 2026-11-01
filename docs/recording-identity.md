# Recording identity

A recording has a stable UUID. A playable source belongs to that recording. A local-file locator belongs to a source and a backend host. Changing tags or moving a file does not prove that its audio changed. Reusing its path does not prove that the recording stayed the same.

```mermaid
flowchart LR
  File[Read-only local file] --> Snapshot[Private verified input copy]
  Snapshot --> Checksum[Exact tagged-file checksum]
  Snapshot --> Decode[Bounded FFmpeg inspection]
  Decode --> Evidence[Versioned audio evidence]
  Evidence --> Resolve[Conservative identity resolution]
  Checksum --> Resolve
  Resolve --> Recording[Stable recording UUID]
  Recording --> Collections[Playlist entries / likes / progress]
  Snapshot --> Metadata[Source metadata observation]
  Metadata --> Display[Selected available source metadata]
```

## Evidence and matching

`local_files.file_checksum` identifies the current file bytes, including tags. An unchanged checksum reuses a completed inspection. A failed inspection is retried on a later scan. A completed unsupported audio profile can reuse its exact-file result.

`recording_evidence` stores an algorithm and digest separately from the recording UUID. The current `ffmpeg-pcm-f64-v1` algorithm hashes decoded 64-bit floating-point PCM together with the codec name, sample rate, channel count, and channel layout. It does not round everything to 16-bit audio. Codec differences remain separate even when a listener might consider them the same performance. This is conservative content evidence, not acoustic fingerprinting or provider matching.

The resolver first checks continuity against the previous source's verified bytes or audio evidence. It then checks exact file anchors and an unambiguous audio match within the same library. Multiple historical candidates remain separate. Matching titles, durations, or paths alone never merge recordings. Replacing audio disables the former source locator while preserving the old recording and personal references.

Tags belong to individual sources. A recording keeps its selected metadata source while that source is available. If it disappears or is replaced, another available local source supplies the displayed metadata. Search updates in the same transaction. When no source survives, Hito keeps the last displayed metadata and clears its owner. Personal overrides and provider provenance are later release work.

## Existing libraries

Migration 0007 preserves every existing recording UUID and collection reference. The old whole-file hash becomes an immutable `initial_file_checksum` anchor. Current file checksums are stored independently.

Before resolving new files, an upgrade scan searches its configured root for unchanged checksum witnesses from the same host and library. Those witnesses can establish audio evidence for existing recordings before directory ordering affects matching. Immutable anchors remain discoverable after their old locator disappears, including on later scans. Existing locators reuse the witness inspection during the normal pass. A newly discovered witness can require a second decode. Evidence-only lookup can find historical roots; filesystem reads remain inside the current configured root.

If every legacy copy was changed before the first upgraded scan, Hito cannot prove continuity from the old checksum. It keeps the old recording and references unavailable and creates a separate recording for the changed file. It does not guess from the reused path. Historical aliases sharing audio evidence are retained rather than silently merged.

## One card for a proven song

Status: source only (migration 0077). It runs in tests against real temporary SQLite and owned fixtures. It has not been deployed or observed on the local server.

An account sometimes holds the same song twice: a local file and an imported streaming track, or two scanned copies. Hito shows one card when it can prove they are the same song. It never guesses.

```mermaid
flowchart LR
  Tags[Tagged MusicBrainz recording id / ISRC] --> Proof[Same identifier in one account]
  Proof --> Checks[Same version wording and compatible length]
  Checks --> Choices[One copy keeps every personal choice and the best audio]
  Choices --> Group[Group row: card + proof]
  Group --> Lists[Library, liked and artist lists show the card]
  Group --> Detail[Recording page lists every copy and its sources]
  Detail --> Split[These are different songs]
  Split --> Memory[Split row: never grouped again]
```

**Proof.** Only two things count: the same MusicBrainz recording identifier from file tags, or the same ISRC (from a file tag or a provider response). Titles and artist names never create a group. A provider source identity is not a third proof yet: an active provider binding is unique per service, so two recordings cannot share one. Pairs that fail any check below stay separate.

**Same version.** The titles must have the same base and the same version wording, using the title rules of artist matching (live, acoustic, remaster, remix, edit and similar). A remix never joins an original. Both lengths must be known and within two seconds, or ten seconds when both carry the same version wording.

**Who carries the card.** Every personal choice and the playable audio must stay reachable from the card. A copy may be hidden only if the other copy has at least its likes, personal title or artist edits and source choice, and at least its audio state (ready audio beats a service binding beats nothing). When each copy holds something the other lacks, such as a like on the streaming copy and the only ready file on the other, Hito leaves both cards. Equal copies pick the lowest identifier.

**Presentation, not deletion.** A group is a row in `recording_group_members` (the account, the hidden copy, the copy that carries the card and the proof). No recording, source, playlist entry, like, progress or bookmark, personal edit, source choice, queue or history row, or download is rewritten, moved or deleted. They keep pointing at their own recording. Merging rows would have lost the identities that playlists and downloads reference, and could not be undone safely. A group is one level deep (a trigger enforces it), at most twelve copies, and belongs to one account because the account's own choices decide it.

**Where it appears.** The library, liked and artist lists hide a copy whose card is another recording; list totals match. A text search still shows every match. The recording page shows **Copies of this song** (title, album, availability, proof, and each copy's sources) from the page data, with no extra request. Album tracklists and playlists still show each recording as stored.

**Undo.** **These are different songs** on a hidden copy removes it from the group and writes `recording_group_splits`. Later passes never group that copy with the rest of its former group again, and a repeated request changes nothing. A recording that carries a card cannot be split from itself; split the other copy instead. Re-grouping after a split is planned, not built.

**When it runs.** As the last step of the account's automatic matching walk, in bounded pages of 400 identifiers, resumable and idempotent. It reads only local rows, so it does not call any service, but it runs under the same job and consent. The log records counts and timing only. Migration 0077 adds two identifier indexes and queues a walk for accounts that already have tagged identifiers.

**Erasure.** Deleting an account removes its group and split rows. Removing a missing recording deletes its group rows, dissolves the group when it carried the card (hidden copies show again) and forgets its splits. Schema-driven tests fail if a table naming a recording or an account is not classified.

**Evidence.** Failing runs came first for each slice: core cases for grouping, versions, lengths, personal choices, splitting and accounts failed on assertions (the list still showed every copy); the two schema-classification tests failed naming the new tables; the HTTP case failed with 404 on the missing routes; the DOM cases failed because the page showed no section. Passing: ten core cases, an HTTP case with a local file and an imported track sharing an ISRC, four DOM cases, a work-budget case (a step costs the same beside 800 or 8,000 foreign identifiers) and the list-read case (hiding 2,000 of 4,000 copies adds about one percent).

**Not built.** Merged playback across copies (the card plays its own sources; another copy is reachable from the recording page), grouping inside album tracklists and playlists, re-grouping after a split, carrying a like to the card, proof from a provider source identity, and live observation. The reported streaming import still needs a provider that supplies ISRCs.

## Inspection limits

The scanner opens its selected music directory once and resolves reads relative to that owned handle through [cap-std](https://docs.rs/cap-std/4.0.3/cap_std/fs/struct.Dir.html). Descendant symlinks cannot grant access outside that directory. Media tools read a private temporary copy whose bytes supplied the checksum. Hito checks the original through the same directory handle before accepting the result. Temporary copies are removed after the last child process releases them. They are inspection files, not playback cache or persistent downloads.

Each inspector shares two file-reader permits and two child-process permits. File reads check cancellation and deadlines every 64 KiB, reject growth/truncation, and hold their permit until blocking work stops. Probe output is limited to 64 KiB; digest output to 256 bytes. Any error-level diagnostic, failed exit, malformed/empty hash, timeout, or cancellation rejects the inspection. Raw diagnostics are never logged.

Directory enumeration streams entries through a 32-path channel without collecting and sorting whole directories. It checks cancellation before advancing even through non-audio files and keeps at most 64 open directory iterators. Deeper trees fail the scan without reconciling missing sources. Stale sources are reconciled in batches of 64, including sources from a displaced configured root on the same host and library. The current configuration has one root; the planned multiple-folder model must reconcile against its complete configured folder set.

Default command timeout is five minutes, with a fifteen-second probe limit. Inputs allow only file/pipe protocols and the supported audio demuxers. Probe allocation and analysis budgets are explicit. Multi-audio-stream inputs, integer PCM above 32 bits, and profiles outside the rate/channel limits receive no decoded-audio evidence and use exact-file matching only.

## Verification and limits

Docker public-capability tests cover retagging with collection references, actual replacement and restoration, scan order and moved-root upgrades, transient decoder failure, metadata-owner fallback, cache reuse, private input ownership, process timeout/cancellation, and prompt shutdown during large-file verification. Controlled tag rewrites cover WAV, FLAC, MP3, Vorbis/Ogg, Opus, AAC/M4A, AAC/ADTS, AIFF, AAC/MP4, and WMA. Separate fixtures distinguish sample-rate changes and a one-bit difference in 32-bit PCM.

These tests exercise the pinned Linux Docker environment and its FFmpeg build. They do not establish native Symphonia compatibility, gapless playback, cross-platform decoder equivalence, acoustic matching, or the public-release performance gates. Decoder changes require compatibility verification before changing the evidence algorithm or accepting new evidence.
