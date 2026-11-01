# Catalog extension contracts

The Apache-2.0 plugin SDK defines read-only artist search, artist profiles, paged discographies, edition listings and complete edition track lists. Artist host and browser integration have separate evidence in the [catalog host](artist-catalog-host.md) and [discography](artist-discography.md) documentation. Edition integration is described below. Existing recording metadata and playlist operations remain separate.

## Negotiation and ownership

A provider declares the existing `catalog` manifest capability. The separate `GET v1/catalog/capabilities` descriptor contains `catalog_version: 1` and a nonempty, unique `operations` list containing `artist_profile`, `artist_discography`, or both. The manifest shape, its serialization and its fingerprints remain unchanged.

Hosts must treat an absent descriptor as an unsupported extension. Unsupported catalog versions and operations must fail closed for this extension while leaving previously supported plugin functions available. Host authorization, bounded HTTP reads and revision checks are not implemented by the SDK types.

```mermaid
flowchart LR
    UI[Hito artist page] --> Catalog[Core catalog orchestration]
    Catalog --> Builtin[Built-in metadata provider]
    Catalog --> HTTP[Approved personal HTTP provider]
    Builtin --> Contracts[Shared typed profile and discography contracts]
    HTTP --> Contracts
    Contracts --> UI
```

This diagram describes intended page ownership. The reviewed-profile host has separate source verification. Providers supply structured data. Hito renders it and never executes downloaded plugin code. These catalog responses have no audio, stream URL, HTML or custom UI fields.

## Recording evidence

`ProviderRecording` (search results, recording details, playlist entries and metadata) has three optional fields that let a host verify who performed a recording. All are omitted from the wire when absent, so earlier plugins and earlier stored responses keep their exact bytes, and a host that predates them rejects them only through the usual unknown-field rule.

- `isrc`: the ISO 3901 code of the recording, 12 characters (two country letters, three alphanumeric registrant characters, two year digits and five designation digits). A plugin may send lower case or the hyphenated display form (`US-RC1-76-07839`); `normalized_isrc()` and `normalize_isrc()` return the canonical upper-case form. A malformed value makes the recording invalid, so a producer omits a code it cannot validate. An ISRC is evidence, not identity: one code can name several recordings, and a recording can be re-released under another.
- `artist_urls`: the canonical pages of **every** credited performer, at most 8, each at most 512 bytes, without repeats and in exactly the form the `url` crate serializes. Only allowlisted shapes are valid: today `https://open.spotify.com/artist/<22 base62 characters>`; any other address makes the recording invalid, so a producer omits pages it cannot place in the allowlist. A provider that cannot name every performer omits the list rather than listing some of them, because a host treats a list of one as "the" artist.

- `artists`: the credited artists as a list, in the provider's order, at most 16. Each entry has a `name` (at most 256 bytes), an optional opaque `provider_artist_id` (at most 128 bytes, no blanks), an optional `page_url` in the same allowlisted form as `artist_urls`, a `role` (`main`, the default, or `featured`) and an optional `join`, the text that follows the artist in the display credit (at most 32 bytes, such as `" & "` or `" feat. "`). A non-empty list needs at least one main artist and may not repeat a name (ignoring case), an identifier or a page. `artist` stays the required display text and is never parsed when a list is present. A provider that does not know the artists as a list omits the field, as every plugin did before it existed. The field is optional and omitted from the wire when empty, so no protocol version changed; a host that predates it rejects it through the usual unknown-field rule.

Hosts must treat all three as claims of the provider. Hito's built-in MusicBrainz lookups use them only as described in [artist matching](artist-matching.md#matching-from-provider-evidence). The bundled folder plugin publishes ISRC tags; the bundled Spotify plugin publishes artist pages, the credited artist list and no ISRC ([why](plugin-spotify.md#artist-pages-and-isrc)). How Hito stores and uses the list is described in [credited artists](metadata.md#credited-artists-from-providers). `RecordingAttribution` is now held behind a `Box` in `ProviderRecording` to keep the playlist item type small; its wire shape is unchanged. `ProviderPlaylistItem::Recording` now boxes its recording for the same reason; the wire shape is unchanged.

## Identity and artist profiles

Provider IDs are opaque, case-sensitive strings scoped to the selected provider connection. Equal strings from different providers are not evidence of equal artists. A host must retain provider provenance and verified associations rather than treating an external ID as a Hito UUID.

## Artist search

Search negotiates independently through `GET v1/catalog/discovery/capabilities`:

```json
{"discovery_version":1,"operations":["artist_search"]}
```

The older catalog descriptor and its operation enum remain unchanged. Adding search there would cause a strict older client to reject the descriptor and lose otherwise supported profile reads. A missing discovery descriptor means search is unsupported; it must not disable those earlier operations.

`POST v1/catalog/artists/search` accepts `ArtistSearchRequest`:

```json
{"query":"Example artist","cursor":null,"snapshot":null,"limit":20}
```

Queries are nonblank text of at most 512 UTF-8 bytes without control characters. Limits range from 1 to 20 candidates. Cursors have a 2,048-byte limit and snapshots a 256-byte limit. Initial requests cannot choose a snapshot. `ArtistSearchPage` returns the exact query, nullable snapshot, candidate items, nullable total count and nullable next cursor. Continuations preserve the exact snapshot, including null. A next cursor requires a nonempty page and must advance; a host must also detect cursor cycles across requests.

Each candidate contains an artist reference, nullable kind and nullable disambiguation. Distinct IDs may have identical display names. Duplicate IDs within one page are invalid. Candidates are suggestions, not confirmed recording-credit associations. Search itself does not adopt recordings, change metadata, download audio or establish canonical artist identity.

Encoded search pages are limited to 65,536 bytes. Validation checks field lengths before scanning their text, bounds item counts before iterating, and counts serialized bytes without allocating another full response buffer. Unknown fields and unsupported discovery versions fail validation. Transport implementations must separately bound incoming bytes before decoding and preserve authorization and provider provenance.

`POST v1/catalog/artists/profile` accepts `ArtistProfileRequest` with `artist_id`. `CatalogArtistProfile` returns the exact requested artist reference, plus nullable sort name, kind, disambiguation and revision. Artist references contain an ID and display name. Kinds are `person`, `group`, `orchestra`, `choir`, `character` and `other`. Missing optional metadata remains unknown.

IDs and revisions are limited to 256 UTF-8 bytes. Names, sort names and disambiguation are limited to 2,048 bytes. Required text must be nonempty and contain no control characters. Complete encoded profiles are limited to 32,768 bytes. Response validation binds the returned artist to the request; the host must call the validator after bounded transport decoding.

## Discographies and continuation

`POST v1/catalog/artists/discography` accepts `ArtistDiscographyRequest` with `artist_id`, nullable `cursor`, nullable `snapshot` and `limit`. Limits range from 1 to 50 release groups. Initial requests have null cursor and snapshot. The initial response may choose an immutable snapshot or return null for a live catalog. Every continuation must send and receive that exact snapshot value, including null.

`ArtistDiscographyPage` returns the requested artist ID, snapshot, release-group items, nullable total count and nullable next cursor. Short pages are valid. Total count may remain unknown; an opaque cursor does not reveal a numeric offset. A non-null next cursor requires at least one item and must differ from the request cursor. IDs must be unique within a page. The host must separately detect repeated cursors across a traversal and bind continuation state to its authorized connection and query.

Release groups represent album concepts. Their IDs are distinct from release editions, track occurrences and recordings. The contract preserves separate same-title groups. Selected-edition contracts are described below; their core, HTTP and browser integration remains unfinished.

Each group contains an ID, title, nullable ordered artist credit, nullable primary kind, secondary type labels, nullable first-release date and nullable revision. Primary kinds are `album`, `ep`, `single`, `broadcast` and `other`. Partial dates retain their precision as `YYYY`, `YYYY-MM` or `YYYY-MM-DD` and must be calendar-valid.

A present credit has 1 to 32 members. Each member retains a nullable identified artist, the credited display name and a join phrase. Repeated identified artists are permitted because a credit is an ordered performance credit, not a set. Unknown artists are not inferred from the credited name. Join phrases may be empty or contain spaces, are limited to 128 bytes and cannot contain control characters. A group has at most 16 distinct secondary labels of at most 128 bytes each.

Cursors are limited to 2,048 bytes. Complete encoded discography pages are limited to 262,144 bytes. Validation counts serialized bytes through a bounded writer and stops on overflow without constructing another full response buffer. All object shapes reject unknown fields. Validation errors are static messages and do not echo provider content.

## Provider-side release-type filter

A provider may page one release type at a time. The separate `GET v1/catalog/discography/capabilities` descriptor contains `discography_version: 1` and exactly one operation, `release_type_filter`. It is independent of the catalog, discovery and edition descriptors, whose bytes are unchanged, so a strict older client never loses profile or discography reads. A missing descriptor means unsupported; hosts must not send a type to such a provider.

`ArtistDiscographyRequest` gains an optional `release_type` of `album`, `ep`, `single` or `other`. An absent type is omitted from the wire, so every earlier request and fixture keeps its exact bytes. `other` means every group that is not an album, EP or single, including broadcasts and groups with no known type. For a typed request, `ArtistDiscographyPage::validate_for` rejects any item whose kind the type does not admit. Cursors, snapshots, limits, size bounds and unknown-field rules are unchanged. A continuation must carry the same type as the request that produced it; the host enforces that, and a provider treats each type as its own paged collection.

## Editions and complete track lists

The SDK defines a separate `v1/catalog/editions/capabilities` descriptor with `edition_version: 1`. Its unique, nonempty `operations` array may contain `release_group_editions`, `edition_track_list`, or both. Existing catalog and discovery descriptor bytes remain unchanged. The host must negotiate each operation independently; edition negotiation is not yet connected to core.

`ReleaseGroupEditionsRequest` carries the exact provider release-group ID, nullable cursor and snapshot, and a limit from 1 to 50. The defined path is `v1/catalog/release-groups/editions`. Its page preserves the group, nullable snapshot, distinct edition identities, optional total and next cursor. Same-title editions remain separate. Continuation validation requires the exact snapshot, including null, a nonempty advancing page and bounded encoded output.

`EditionTrackListRequest` carries both edition and release-group IDs. The defined path is `v1/catalog/editions/track-list`. Its result retains the exact identities, title, optional credits/revision and ordered media. Each medium has ordered track occurrences with its own identity, display number, kind, optional duration and optional provider recording reference. Repeated recording references are valid; repeated occurrence identities are not. Only pregaps use position zero. Data tracks remain distinct from audio.

```mermaid
flowchart LR
    Group[Release group] --> Edition[Selected edition]
    Edition --> Media[Ordered media]
    Media --> Occurrences[Track occurrences]
    Occurrences -. optional reference .-> Recording[Provider recording identity]
    Recording -. resolved separately .-> Sources[Playable sources]
```

Generic provider IDs remain opaque strings of at most 256 UTF-8 bytes. Track lists allow at most 256 media, 4,096 occurrences and 262,144 encoded bytes. Positions must increase; the generic contract permits sparse numbering. Unknown credits, dates, durations, revisions and recording identities remain unknown. Listing metadata never establishes playable availability or adopts recordings.

The built-in MusicBrainz adapter now implements these reads through its existing paced, bounded, fixed-origin transport. It validates canonical MusicBrainz IDs and exact returned group/edition identities, advances short browse pages by their actual returned count, and exposes no invented snapshot or revision. Complete track lists validate normal counts separately from pregap/data occurrences. Leading data tracks follow the upstream [medium serializer](https://github.com/metabrainz/musicbrainz-server/blob/master/lib/MusicBrainz/Server/WebService/Serializer/JSON/2/Medium.pm), preserving their original positions.

Five edition HTTP fixtures and five named SDK contract/compatibility cases pass, including independent repeats. Strict default/schema checks and final source reviews pass. The tests cover explicit edition selection, short pages, repeated recordings, pregaps/data tracks, incomplete lists, mismatched identities, unchanged older wire bytes and safe logs. No dependency selections change. Core authority and continuation, HTTP plugin negotiation, authenticated application routes, generated contracts, browser navigation and live MusicBrainz acceptance remain open.

## Verification and remaining work

All 54 SDK cases pass with generated-schema support, and strict all-target SDK Clippy and formatting pass. The nine search cases cover negotiation, unchanged legacy descriptors, same-name distinct identities, continuation state, UTF-8 and nested bounds, exact escaped 65,536-byte pages, generated schemas and allocation rejection. Compatibility cases preserve exact earlier manifest, recording and playlist wire bytes. Earlier profile/discography cases cover nullable unknowns, ordered credits, identity mismatch, live and snapshot continuations, partial dates and exact encoded limits.

An allocation regression constructs a field-valid oversized page before observing public validation. Before the fix, validation requested a 16,777,216-byte allocation. The unchanged test now verifies that no single validation allocation request exceeds the 262,144-byte page budget. It calibrates its observer and does not measure RSS, total allocation volume or peak live memory. Final security, performance and correctness reviews pass after the bounded-writer fix.

The search allocation case separately calibrates its thread-local observer and rejects an encoded page larger than 128 KiB without a single validation allocation request exceeding 65,536 bytes. Independent search correctness and security reviews pass. The performance review found text trimming before length rejection; the length-first fix passes its second review and all 54 unchanged cases. These SDK checks do not establish provider transport, browser or release performance acceptance.

Core source separately retains [raw artist tag assertions](metadata.md#musicbrainz-artist-tag-evidence-in-verified-source). The [reviewed-profile host](artist-catalog-host.md) adds account settings, explicit evidence associations and authorized profile reads. Subsequent [discography work](artist-discography.md) records artist-page browsing. Edition host/browser integration, automatic enrichment, caching and optional similar artists remain unfinished. These source foundations do not satisfy the complete release requirement.
