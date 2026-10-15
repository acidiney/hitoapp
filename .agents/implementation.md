# Active implementation

The objective remains the end-to-end deployed Hito system in docs/release-plan.md. The public release is unfinished. Do not mark the full goal complete after a development slice.

## User requirements

Work and verify in Docker. Keep Rust and React cohesive, maintainable, and testable through public boundaries. Use TDD, minimal comments, safe correlated logs throughout workflows, clear documentation, and diagrams where useful. No shared forwarding facade, route-switching mega-component, fabricated library content, or silent reduction of release scope.

Make small focused commits with a single-line message and signing explicitly disabled using `git -c commit.gpgsign=false commit --no-gpg-sign -m ...`. Do not change global Git configuration or push. Ignore generated audio, screenshots, traces, databases, and backups. Most folder changes have separate module/test/UI/setup commits; one coupled core wiring commit, 85a01e3, is larger. Do not claim every commit is tiny.

## Current state

Capability services own identity, catalog, collection transactions, and library storage. Rust routes generate OpenAPI and TypeScript. The shared React UI has focused pages/hooks, a recording spotlight, artwork grid/list, personal playlists, grouped navigation, listening sidebar, persistent player, expanded queue, and tabbed Settings. DESIGN.md records the user's visual references. No generated catalog music or artwork is presented as user content.

Recording identity is separate from tagged-file checksums. Migration 0007 retains stable UUIDs and immutable legacy anchors. Versioned FFmpeg f64 PCM evidence supports conservative matching and independent source metadata observations. Ambiguous historical IDs stay separate; all-changed legacy libraries cannot prove continuity and keep historical references unavailable rather than guessing by path. Ten container fixtures and precision, witness-order, ownership, retry/cache, cancellation, and private-input regressions pass. See docs/recording-identity.md.

## Multiple music folders

Implemented migration 0008, trusted startup directory grants, administrator-only directory browsing and folder connection/removal/rescan, per-folder durable jobs, reconnect preservation, host ownership, lossless native file locators, owned playback files, and private range streaming. `LibraryStorage` owns those rules and private SQLite; HTTP remains thin. Shared config can come from a bounded versioned HITO_STORAGE_MANIFEST; the legacy HITO_MUSIC_ROOT path still works.

Every scan write checks its attempt token, unexpired lease, folder generation, and connected binding. Removal cancels scans and withdraws sources without deleting originals or personal references. Root and file access use owned cap-std handles. Source metadata falls back to another available owner. Legacy witness discovery checks pending evidence once before opening roots, and canceled witnesses are supervised independently.

Per-grant opening/browsing permits and five-second deadlines isolate filesystem stalls. Walkers use a separate grant permit, bounded channel, explicit success/failure termination, and progress notifications including unsupported entries. Five seconds without progress fails enumeration and preserves previous verified sources. Detached threads retain permits until the actual operation finishes and do not hold Tokio runtime shutdown. Operating-system calls cannot be forcibly canceled in process.

React Settings offers a library selector, folder status/scans, removal confirmation, and a directories-only native dialog for already granted shares. Hooks scope requests and mutations to account/library, cancel stale work, serialize changes, and poll active jobs. Closing restores focus. Ordinary listeners never request storage administration. An empty catalog links to folder Settings. Library scan waits for all folder jobs.

Development host launchers are Hito Setup.command, Hito Setup.ps1, and hito-setup. They use native folder choosers and Docker-only JSON generation for read-only mounts. Existing recognized volume/project/port and legacy music mounts are adopted; ambiguous discovery stops. Installation files are private, Compose dollar interpolation is escaped, and stopped-volume backups precede startup. The browser has no Docker socket/control API. Normal setup does not require HITO_MUSIC_FOLDER editing. See docs/music-folders.md and README.md.

Native launcher dialogs, host permissions, drive reconnects, repository moves, discovery ambiguity, backup/rollback behavior, and packaged installation still need live acceptance on macOS, Windows, and Linux. PowerShell syntax parsing in Docker is not Windows GUI verification. Automatic desktop Music-folder discovery remains planned.

## Artists and albums

Implemented Catalog-owned album/artist indexes and details, JSON/Inertia routes, source-based release appearances, disc/track positions, exact library-scoped artist IDs, FTS5 Unicode prefix search, release artwork, and shared React pages. Same recording can appear on multiple releases and at distinct positions. Duplicate copies at the same position collapse. Album rows use occurrence IDs; playback still resolves stable recording IDs. Above 60 tracks the UI explicitly offers Play this page; playback across all pages remains unfinished.

Migrations 0009 through 0012 add positions and metadata cache version, append-only exact credits, Unicode search, and indexed source-to-credit links. The sole observations::remember writer binds credits atomically after its metadata upsert and checks one affected row. Legacy adoption uses it too. Old cached metadata refreshes on the next ordinary scan without changing the audio-evidence algorithm. Credit identity is BINARY and library-scoped, not canonical-person matching. Release identity still includes its parent directory. See docs/catalog-browsing.md.

Two review cycles found release cover collisions, lost paging filters, non-ASCII search failures, global metadata scans, row artwork using recording URLs, and compilation-heavy artist-by-album lookups. Final fixes are in source with regressions; no third independent review. The resolver inspected 0012 ownership. Final diagnostic /tmp/hito-browse-query-fixed-final.log uses actual Cargo SQLite/migrations/current SQL constants and synthetic owned WAL data. Counting 10,000 credits across 100,000 tracks improved from an eight-second abort to about 66 ms; a selected page is about 20 ms after ANALYZE. Explicit indexed join order avoids the after-ANALYZE regression. These are development diagnostics, not release latency benchmarks.

A permanent public Catalog API smoke test seeds a compilation-heavy 100,000-track fixture and checks bounded outputs before and after ANALYZE under a generous five-second read deadline. The fixture seeds normalized credit links because raw SQL bypasses the application writer. Public retag, artwork-isolation/removal, Unicode/literal input, occurrence, and HTTP permission tests pass. Browser screenshots are test-results/albums-desktop.png, album-desktop.png, and artist-mobile.png. The finish reviewer gave screenshot/source PASS with no live keyboard/performance certification.

## Verification and reviews

Latest full `scripts/verify.sh` passed 92 Rust workspace tests, one separately run enumeration fault test, 36 frontend tests, two setup CLI tests, formatting, Clippy with warnings denied, generated contracts, production build, zero audit vulnerabilities, shell syntax, Compose interpolation, and Chromium workflow. Log: /tmp/hito-browse-verified-complete.log. PowerShell syntax parse passed separately in the official image, /tmp/hito-setup-powershell-parse.log.

The browser uses fixtures/e2e-generated and a temporary database, never the normal persistent server. It connects two folders, waits for scans, checks phone layout, Escape/focus restoration, playback, and account isolation. Inspected artifacts: test-results/folders-desktop.png and test-results/folder-picker-mobile.png. Artifacts are ignored. Earlier identity tests accidentally shared a fixture mount containing personal audio; isolation was corrected without removing originals.

Foundation, UI/artwork, identity, folders, and artist/album browsing each received two independent review cycles. Final-cycle fixes received regression/full checks without a third review. Final identity/folder privacy reviews were source-only. Folder findings included expired-attempt reconciliation, unsafe route logging, byte-range edge cases, filesystem supervision, repeated witness queries, and Compose interpolation/project discovery. All were resolved with the limits in .agents/meta/review-chain-report.md. No third folder review should be claimed or launched just to certify final patches.

Important evidence correction: a reviewer retracted its alleged production readdir64 stall reproduction. Production rustix uses raw syscalls, which the preload shim did not intercept. The source-level supervision defect was fixed. The replacement enumeration regression explicitly sets RUSTFLAGS=--cfg rustix_use_libc and runs the normally ignored storage_walk_stalls test separately. Root-open injection exercises the normal Linux backend. Neither proves production raw-syscall filesystem outages or native platform behavior.

Query-plan checks on 100,000 duplicate locators avoid sorting in checksum, metadata ownership, and reconciliation lookups. These are not release latency benchmarks. Rust 1.99 is pinned across project/Docker/editor; real rust-analyzer LSP expanded tracing::instrument. Idle SIGINT/SIGTERM and Docker stop have regression evidence; active long-stream drain remains a release gate.

## Deployment

Normal http://localhost:8080 is healthy after migration 0012. It retains volume hito_hito-data and the saved backend identity. Backup backups/hito-before-catalog-browsing-20261015T174116Z.tar.gz precedes the update. A read-only Docker SQLite comparison preserves 1 account, 7 recordings, 7 sources/local-file references, 1 playlist, 2 entries, 1 like, 6 progress rows, membership, checksums, and backend identity. All 7 source-credit links backfilled, foreign keys pass, and one existing folder is connected/bound. Health is ready; setup remains false. No authenticated production rescan was triggered. Never read credentials or session hashes to bypass authentication.

Older backups remain: hito-before-music-folders-20261015T165652Z.tar.gz, hito-before-recording-identity-20261015T151001Z.tar.gz and hito-before-foundation-20261015T132403Z.tar.gz. Backup directory is 0700; all archives are now 0600. No signing credentials or provider accounts are configured. A private temporary verification script is /tmp/hito-catalog-migration-check.py; it prints counts/booleans only.

## Remaining work

TODO.md is the actual checklist. Continue the approved release plan without requesting approval again for existing requirements. Current folder behavior is implemented and locally deployed; platform acceptance is still a release gate, not completed by fixture tests.

The rest includes metadata provenance and provider-backed matching, MusicBrainz and configurable enrichment, provenance/personal corrections, three Tauri desktop clients and native playback, durable offline replicas/import, explicit downloads, HTTP sources/relay/conversion, four debrid adapters, reference provider deployments, RSS/audiobooks, lyrics, community service, performance/recovery/packaging/signing/restore/update acceptance. Most remain unimplemented. External release dependencies are provider test accounts, hosting/DNS, Apple signing material, and Windows signing acceptance.
