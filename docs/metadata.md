# Personal metadata and source observations

Track details let listeners edit a recording's title and artist. Album pages link to a separate editor for that album's title and album artist. Each field has its own **Use source** action. Edits belong to the signed-in account and do not change audio files or source observations.

## Fields and identity

Recording edits follow the recording UUID across library search, likes, playlists, listening history, and every album appearance. Album edits follow the release UUID. Changing an album artist leaves its track artists alone. Giving two albums the same title and artist does not merge them.

Saving a value pins it, even when it equals today's inherited value. Retagging or selecting another source does not replace the pin. Reset accepts a new synchronization operation and follows inherited metadata again. Library rows use the selected source; album track rows use each appearance's observed title and artist.

The browser updates current and queued recording fields without reopening audio or changing position or speed. Each queued occurrence carries its inherited title and artist, so reset can restore different appearance credits. Queues remain device-local snapshots. Updating them after an external source change or an edit from another client still needs the synchronization client.

Artist pages group exact credit text within an account and library. These are credit labels, not canonical people. A personal artist never enters the shared observed-artist registry. Its account-owned ID stays stable if a later file inspection observes the same name. Empty historical credits are retained internally but hidden from browsing.

Raw retagging can still change an inferred release UUID because its key includes album tags and directory identity. Personal edits do not run that identity constructor. Moving corrections to a different release after raw retagging needs explicit reconciliation and remains unfinished.

## Acceptance and access

The versioned synchronization API accepts these field operations:

| Object | Set | Follow source |
| --- | --- | --- |
| Recording title | `recording_title_set` | `recording_title_inherit` |
| Recording artist | `recording_artist_set` | `recording_artist_inherit` |
| Album title | `release_title_set` | `release_title_inherit` |
| Album artist | `release_artist_set` | `release_artist_inherit` |

Each field retains its own accepted operation UUID. Identical delivery acknowledges the original operation without reapplying it over a newer reset. Reusing a UUID with different contents fails. Batch acceptance, personal state, search indexes, artist relations, and the account journal commit or roll back together. Conflicting field operations follow committed server acceptance order.

Values require 1 to 2048 UTF-8 bytes, nonblank text, and no control characters. Live library membership gates new writes and metadata reads. Revocation hides retained personal state and artist relations immediately. Regrant restores the account's state and refreshes its catalog transactionally. Other accounts receive their own effective values and operation IDs.

## Provenance

`GET /api/v1/recordings/{id}/metadata` returns effective, inherited, and personal values for both fields, their operation IDs, the selected source observation, and a bounded observation page. `GET /api/v1/releases/{id}/metadata` returns the album fields and observations associated with that release. An album does not claim one selected track observation for its entire group. Neither response exposes filesystem paths or file checksums.

Each successful inspection records a normalized snapshot and six supported chosen tags: title, artist, album, album artist, disc, and track. Tag values retain whether FFprobe found them in file tags or an audio stream. Missing tags remain absent. Normalized values still apply Hito's filename and unknown-value fallbacks.

A distinct source/checksum/inspection revision gets one immutable snapshot. Cached scans reuse it. Returning to a previously observed revision can select its earlier snapshot; this is revision evidence, not a log of every scan. Personal edits never rewrite these snapshots.

Migration 0014 records available upgrade metadata as `legacy_normalized`. Original tags and observation times remain null. Inspection version 2 captures supported tag evidence on the next successful scan. Migration cannot reconstruct a previously discarded source pointer.

Current source metadata points to its observation. Recordings keep separate selected-observation pointers. If every source becomes unavailable, Hito retains the last inherited metadata and observation while clearing the playable source owner.

History pages are newest first, default to 20 entries, and return at most 100. The exclusive `before` cursor is constrained to the authorized object. A recording's selected observation is returned separately, even outside that page. The browser replaces one page at a time and cancels stale requests across account, object kind, and object ID changes.

## MusicBrainz artist tag evidence in verified source

Inspection version 3 retains emitted `MusicBrainz Artist Id`, `MUSICBRAINZ_ARTISTID`, `MusicBrainz Album Artist Id` and `MUSICBRAINZ_ALBUMARTISTID` tags. `InspectedTags.musicbrainz_artist_tags` contains their exact key, raw value, performer or album-artist role, and format/audio-stream location. Values remain source assertions. [Artist matching](artist-matching.md) uses a single, valid performer value as verified evidence; provider enrichment remains separate work.

A null array means evidence was unavailable, including old observations and rejected optional evidence. An empty array means inspection found no recognized artist tag. Values are retained without UUID parsing or delimiter splitting. The collector permits at most 16 assertions, 64 bytes per key, 2,048 bytes per value and 32 KiB for the encoded array. Exceeding a bound makes the optional evidence unavailable.

Expanded FFprobe output remains limited to 65,536 bytes. A clean output-limit failure permits one ordinary-tag probe under the same original deadline, including admission and cleanup. Diagnostics, an observed unsuccessful exit, cancellation, timeout or malformed JSON do not trigger another attempt. Captured processes and private inputs stay owned until retirement. Logs contain controlled reasons, counts and timings rather than values or paths.

On the next scan, unchanged version-2 files refresh metadata while retaining completed decoded-audio evidence, including known unsupported profiles. Current version-3 inspections are reused. Original media, old observations, recording/source IDs and private collection state remain intact.

This slice was not yet on the local server when it was tested; the local development server now runs through migration 0069. Public fixtures verify raw assertions, tag-only changes, a truthful version-2 upgrade without redecoding, 70 KiB and 1 MiB optional values, shared deadlines, diagnostics combined with overflow, a proven natural failed exit, cancellation and private log values. The frozen full core run passes 316 distinct cases with one usual filesystem-fault case ignored; the later 1 MiB case passes separately. The server passes 144 distinct cases. Strict workspace Clippy, formatting, generated contracts and frontend checks pass. These Linux fixtures do not establish every container/tag format or other platform's process behavior.

## ISRC tags in verified source

Inspection version 4 also retains the first `isrc` (ID3 `TSRC`) tag as `InspectedTags.isrc`, a raw source assertion like the ordinary tags (null when absent or not yet inspected). Its value is normalized only when Hito uses it as [provider evidence](artist-matching.md#matching-from-provider-evidence); a malformed value is ignored there. Unchanged version-3 files refresh their tags on the next scan without decoding audio again, exactly as version 2 did. FLAC, Ogg Vorbis, MP3, M4A and WAV (ID3 chunk) fixtures verify the tag; other containers are unverified.

## Release and recording MusicBrainz identifiers in verified source

Inspection version 5 also retains four more tags as raw source assertions on `InspectedTags` (each null when absent or not yet inspected): `musicbrainz_album_id` (`MusicBrainz Album Id`, `MUSICBRAINZ_ALBUMID`, the release), `musicbrainz_release_group_id`, `musicbrainz_recording_id` (`MusicBrainz Track Id`, `MUSICBRAINZ_TRACKID`, the recording, following MusicBrainz Picard) and `musicbrainz_release_track_id` (`MusicBrainz Release Track Id`, `MUSICBRAINZ_RELEASETRACKID`). Unchanged version-4 files refresh their tags on the next scan without decoding audio again.

Each well-formed value (a hyphenated UUID; stored in lower case; the nil UUID is rejected) is also kept as an immutable row in `recording_musicbrainz_observations` (migration 0064), beside the observation that stated it, with the recording and the release the observation belongs to. A malformed value stays a raw assertion only. Rows are never updated or deleted, so a retag adds a new observation instead of changing an old answer.

This is evidence, not a merge. Two local releases that carry the same release identifier stay two releases, a recording is never matched to another by an identifier, and nothing here edits a title, artist, album or any other personal field. The rows are not shown on album or recording pages yet; they are visible only as the raw tags of the observation. The release and release group rows drive cover art and genres, and the recording row is evidence for the credited artist in the background provider pass (migration 0076, see [artist matching](artist-matching.md#release-and-recording-identifiers-from-tags)); reading the tags never contacts MusicBrainz.

FLAC, Ogg Vorbis, M4A (freeform atoms), MP3 and WAV (ID3 chunk) fixtures verify the tags. A `TXXX` frame named `MusicBrainz Track Id` is ambiguous (some taggers put the release track identifier there), so on files with ID3 tags (MP3, WAV, AIFF, ADTS) that value is deliberately not read as the recording identifier. Vorbis and MP4 `MUSICBRAINZ_TRACKID` / `MusicBrainz Track Id` are read as the recording identifier. Files with ID3 tags state the recording in the `UFID` frame; see inspection version 7.

Inspection version 6 also retains the first `genre` tag as `InspectedTags.genre`, a raw source assertion (null when absent or not yet inspected), and derives the browsable genre list from it (see [catalog browsing](catalog-browsing.md#genres-and-artist-radio)). Unchanged version-5 files refresh their tags on the next scan without decoding audio again, so the first scan after the 5 to 6 upgrade re-probes every file once. Recordings without a file genre can also take MusicBrainz genres (see [artist matching](artist-matching.md#musicbrainz-genres)); these are separate observations with provenance `musicbrainz` and never replace the tag or a personal edit. FLAC, Ogg Vorbis, MP3, M4A and WAV (ID3 chunk) fixtures verify the tag; the browser fixtures verify the WAV INFO `IGNR` form.

Inspection version 7 reads the ID3 `UFID` frame owned by `http://musicbrainz.org`, where MusicBrainz Picard stores the recording identifier in MP3 and WAV (ID3 chunk) files, as `musicbrainz_recording_id`. FFprobe does not expose that frame, so Hito reads it itself from the private media copy while it already prepares the ID3 tags for FFmpeg. The reader is bounded: the tag span is checked against the file and the existing 16 MiB tag limit, the first MusicBrainz-owned frame decides (when its identifier is unusable there is no recording identifier, and later frames or stacked tags are not consulted, as Picard and mutagen do), the owner must match exactly, the identifier must be 1 to 64 printable ASCII bytes, and a frame that is compressed, encrypted, grouped, individually unsynchronised or prefixed with a data length is skipped (compressed text frames remain supported; a compressed `UFID` is not). Whole-tag unsynchronisation, extended headers and footers are handled, and malformed headers leave the value empty without failing the scan. The value is a raw source assertion like the others; only a well-formed UUID becomes evidence. No network request is made. Unchanged version-6 files are refreshed on the next scan without decoding audio again, but every file is still copied to the private media area, hashed and probed once, so the first scan after the upgrade reads the whole library from disk. Existing MP3 and WAV libraries gain the identifier and the background artist matching and proven-duplicate grouping see it without a migration. Fail-closed limits, each leaving the recording identifier empty: ID3v2.2 tags (three-letter frame ids), tags appended at the end of a file, tags with more than 8,192 frames, tags larger than 16 MiB, identifiers padded with NUL bytes (the identifier must be printable ASCII), and formats that do not carry ID3 (FLAC, Ogg, M4A keep their own tags). AIFF and ADTS files with ID3 tags use the same reader but have no fixture. Tests cover both tag versions, RIFF chunks, stacked tags, unsynchronisation, tables of rejected frames, first-frame cases, extended headers, footers, RIFF chunk edge cases, and truncated and corrupted headers with pinned outcomes (unit), scans of MP3 and WAV through the public capability, and the upgrade from version 6 through to an artist match.

One-time cost of the version 4 to 5 change. Version 5 is a new inspection version, so the first scan after an upgrade re-probes every file with FFprobe once. Audio is not decoded again. Each file gains one observation row and at most four evidence rows. A large library therefore takes noticeably longer on that first scan; later scans reuse the version 5 result for unchanged files.

## Compressed ID3 tags and tag compatibility

FFprobe reads tags from a private copy of each file. Before it runs, Hito looks at ID3v2.3 and v2.4 tags at the start of the file and in RIFF (WAV) and AIFF ID3 chunks. Only tags that contain at least one compressed frame are rewritten, in place and at the same length. Other files are untouched.

Behavior before this change, measured with FFmpeg 5.1 in the Docker image:

| Case | Result |
| --- | --- |
| v2.4 compressed text frame | Read correctly |
| v2.3 compressed text frame | Flag not recognized. The field fell back to the filename or showed garbage |
| Corrupt zlib stream, or a stream larger than FFprobe's 16 MiB allocation limit, in v2.4 | FFprobe printed an error and the whole file was not indexed, although the scan finished |
| v2.3 tag with whole-tag unsynchronisation and a compressed frame | Other frames in the tag were lost as well |

Current policy:

- Only compressed `T***` text frames are inflated. Other compressed frames, including pictures, are dropped from the private copy. Frames that also carry grouping or encryption flags are dropped.
- A frame is dropped, never fatal, when its declared size is zero or above 1 MiB, when the file's declared total passes 4 MiB (charged before inflating, across stacked tags and chunks), when a frame above 4 KiB expands more than 100 times, when the stream is invalid or its length differs from the declared size, or when the plain result does not fit in the original tag. v2.4 tag-level unsynchronisation applies to every frame and footers are skipped. A failed write to the private copy is counted and the copy is used unmodified. Reading stops at 16 MiB of tag, the snapshot deadline applies, and decompression never allocates beyond the declared and capped size.
- Dropped frames leave the field to the filename or unknown-value fallback, so observation provenance still shows the field as not found in tags. Personal edits and pins are unchanged.
- Per file, one debug event `id3_compressed_frames` records counts of inflated and dropped frames by reason. It contains no values or paths.
- In WAV files that hold both a `LIST`/`INFO` chunk and an ID3 chunk, FFmpeg ignores the ID3 tag. Hito renames the INFO list in the private copy so the ID3 tag wins. Its fields are fuller (album artist, disc). Fields missing from the ID3 tag are not filled from INFO.

Unchanged files keep their earlier observation until they change; there is no forced rescan. Embedded artwork still uses a separate reader that does not decompress ID3 frames.

Tests build tags byte by byte and scan them through the public capability. They cover compressed title, artist and album in v2.3 and v2.4, on MP3 and WAV, per-frame and whole-tag unsynchronisation, corrupt streams, oversize and highly expanding frames, false declared sizes, non-text compressed frames, privacy of logs and personal edits. A compatibility test writes the title, artist, album, album artist, disc and track with Lofty, FFmpeg or hand-built bytes and reads them back for these dialects: ID3v1, ID3v2.3, ID3v2.4, AAC with ID3, FLAC, Ogg Vorbis and Opus comments, M4A/MP4 atoms, WAV `LIST`/`INFO` and ID3 chunks, AIFF ID3 and WMA. ID3v1 and INFO carry no album artist or disc. FFmpeg ignores an ID3v1 tag when any ID3v2 tag exists, even an encoder-only one. ID3 tags appended at the end of a file are not read. These are Linux Docker results with FFmpeg 5.1 and do not cover other FFmpeg versions.

## Ownership and data flow

```mermaid
flowchart TD
    File[Owned file inspection] --> Observation[Immutable tag observation]
    Observation --> Source[Current source metadata]
    Source --> Selected[Selected inherited recording fields]
    Listener[Authenticated field operation] --> Collection[Collection transaction]
    Collection --> Overrides[Sparse account recording and release state]
    Collection --> Journal[Account synchronization journal]
    Source --> Projection[Account artists, releases and performer relations]
    Overrides --> Projection
    Grant[New library membership] --> Projection
    Selected --> Search[Sparse effective recording search rows]
    Overrides --> Search
    Projection --> Catalog[Membership-gated catalog reads]
    Search --> Catalog
    Catalog --> UI[Library, album, artist and detail pages]
```

Library storage owns inspection, observations, source selection, and indexing transactions. Identity provisions account relations when granting a library. Collection owns personal operation acceptance. The private metadata module owns validation, effective field expressions, catalog projection updates, and bounded provenance reads. HTTP authenticates and maps typed results. React uses generated contracts.

Recording search state stays sparse. Only edited or reset recordings need replacement rows. The effective title and artist are indexed together; inherited updates refresh them in the same transaction. Search merges those rows with the remaining shared branch. Album and artist graphs are materialized per account, with account-scoped FTS and indexed performer relations. Every read still checks current library membership. Unchanged indexing skips projection and FTS rewrites.

Migration 0015 preserves prior title pins, resets, and operation IDs while adding an independent recording artist field. Migration 0016 backfills the account catalog. Migration 0017 adds independent release fields and indexed release history.

## Evidence and limits

Public Docker tests cover field propagation, private search and grouping, equal-name album separation, grant/revoke/regrant, rollback, replay, retags, appearance fallbacks, source changes, retained provenance, paging, cache reuse, and truthful upgrades. A schema-14 upgrade test preserves saved pins and reset operation IDs through the public capability. HTTP tests cover authentication, authorization, rollback, private values, and Inertia props. Log capture rejects private titles, credits, invalid route identifiers, and credentials.

The Chromium workflow saves and resets all four fields while audio continues, verifies the unchanged album URL and track performers, and follows the personal album artist. It delays a real refresh response to verify disabled controls and preservation of an unsaved artist draft. Desktop and phone captures were inspected; a phone viewport assertion keeps the album reset control above the player dock. These checks do not establish native or cross-browser acceptance.

Independent diagnostics used bundled SQLite 3.46.0 with 100,000 recordings, ten accounts, 50,000 recording correction states, 10,000 release correction states, and one million account source credits. After `ANALYZE`, median count/page timings were about 29/0.25 ms for recording browse, 41/77 ms for broad recording search, 21/9 ms for artist browse, and 4/0.62 ms for release browse. A new membership across 100,000 sources took 861 ms. Unchanged source refresh across ten or eleven memberships took 0.067–0.094 ms and wrote no rows. Indexed history returned 101 rows in 0.13–0.22 ms. Migration 0016 took about six seconds. These sequential synthetic measurements exclude HTTP, concurrent listeners, decoding, and playback latency.

Provider enrichment, shared correction layers, negative matches, split, general undo, disc/track corrections, stable corrected appearance identities, administrator promotion, and release identity reconciliation remain unimplemented. Personal unlinking and latest-change undo exist only for reviewed artist-catalog associations; see [Reviewed artist profiles](artist-catalog-host.md#unlinking-and-undo).
