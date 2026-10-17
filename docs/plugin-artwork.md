# Provider artwork in development

Hito can retain provider covers in its private SQLite library. This capability is implemented and tested in an isolated branch. Automatic host execution, HTTP endpoints, UI requests, existing-copy backfill, and deployment are still in progress.

The plugin SDK declares an `artwork` capability and a typed `POST /v1/artwork` request containing an entity kind and provider identifier. Plugins return PNG or JPEG bytes. They do not return a browser URL or supply UI components. The reference Spotify implementation supports recording, release, and playlist requests without Spotify account authorization.

```mermaid
sequenceDiagram
    participant Caller
    participant Catalog
    participant SQLite
    participant Plugin
    Caller->>Catalog: Enqueue artwork for an owned recording
    Catalog->>SQLite: Check session, metadata owner, quotas; save receipt
    Catalog-->>Caller: Stable queued or retained receipt
    Note over Caller,Plugin: Enqueue and status reads make no provider request
    Catalog->>SQLite: Worker claims a session-bound lease
    Catalog->>Plugin: Bounded binary artwork request
    Plugin-->>Catalog: Original image bytes
    Catalog->>Catalog: Decode and check framing and dimensions
    Catalog->>SQLite: Recheck authority; publish cover and ready receipt atomically
    Caller->>Catalog: Read retained artwork
    Catalog-->>Caller: Authorized private bytes
```

## Ownership and retention

Artwork belongs to an account and recording. A fetch pins the selected provider metadata observation, binding incarnation, installation revision, service revision, and initiating session. A worker checks that authority during retrieval and again in the publication transaction. Logout or withdrawn authority prevents publication. Network and decoding work happen outside the write transaction.

Local file artwork remains preferred. A retained provider cover remains readable offline and after the plugin is removed. Retrying a completed request for the same selected metadata owner returns its original ready receipt. Each retained cover protects that exact receipt from history cleanup. Changing metadata ownership can require a fresh fetch while the previous cover remains readable.

Retrieval does not reimport a playlist, adopt a recording, download audio, or change playlist entries, likes, progress, or personal metadata. Independent copies and duplicate occurrences keep their identities.

## Bounds and lifecycle

- Encoded images are at most 2 MiB, with dimensions from 1 to 1,024 pixels and an output buffer of at most 8 MiB.
- PNG decoding checks critical and ancillary CRCs through the end of the input and rejects animation. JPEG decoding uses the locked zune strict mode, at most 64 progressive scans, and exact consumed-input and EOI checks.
- Decoder limits bound specific buffers and work. They do not establish a total process allocation or decoding time limit. The JPEG dependency can still pad some prematurely terminated entropy data; these checks do not prove rejection of every malformed JPEG.
- There are at most 16 pending requests per account, 128 globally, one active lease per account, and four globally. Background provider admission is separate from interactive requests. Local decode permits are acquired after the image download.
- Image assets are limited to 32 MiB and 1,024 distinct assets per account, or 256 MiB and 8,192 globally. Pending requests reserve their maximum encoded size.
- Fetches allow three attempts with a 60-second lease. Provider transport has a ten-second request deadline. Read-only authority checks have a one-second deadline. Publication transactions are awaited rather than abandoned on timeout.
- Joined shutdown requeues interrupted work without consuming an attempt. Ready covers remain retained. Disposable terminal history expires after seven days and has account and global caps. Current retained receipts grow with the recording library.

Status polling uses a scoped read transaction and does not run global cleanup or acquire the SQLite writer. Cleanup still runs from enqueue and worker paths. Its work at the full release load remains to be measured.

## Verified evidence and remaining work

Eleven focused public core cases pass in Docker, alongside strict all-target Clippy. They cover private retention across independent copies and restart, unchanged collection state, logout and provider removal during held image bodies, joined shutdown, corrupt PNGs, incomplete JPEGs, retained-receipt cleanup, unrelated writer contention, and interactive searches and local covers during four background downloads.

The preceding full core snapshot passed 179 uniquely identified cases, with the usual filesystem fault case ignored. The subsequent JPEG and read-only status fixes passed the focused suite and Clippy. These totals overlap and must not be added together.

Two independent review rounds found issues that received failing regressions and passing resolver checks. There is no third independent clean verdict. Broad image compatibility, crash and disk-exhaustion behavior, the 100,000-track performance gate, automatic backfill, and release acceptance remain open. See [the complete release scope](release-plan.md).
