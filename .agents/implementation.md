# Active implementation

The objective remains the end-to-end deployed Hito system in docs/release-plan.md. The public release is unfinished. Do not mark the full goal complete after a development slice.

## User requirements

Work and verify in Docker. Keep Rust and React cohesive, maintainable, and testable through public boundaries. Use TDD, minimal comments, safe correlated logs throughout workflows, clear documentation, and diagrams where useful. No shared forwarding facade, route-switching mega-component, fabricated library content, or silent reduction of release scope.

Make small focused commits with a single-line message and signing explicitly disabled using `git -c commit.gpgsign=false commit --no-gpg-sign -m ...`. Do not change global Git configuration or push. The previously untracked codebase now has focused commits; ignore generated audio, screenshots, traces, databases, and backups.

## Current state

Capability services own identity, catalog, collection transactions, and indexing. Rust routes generate OpenAPI and TypeScript. The shared React UI has focused pages/hooks, a recording spotlight, artwork grid/list, personal playlists, grouped navigation, listening sidebar, persistent player, expanded queue, and tabbed Settings. DESIGN.md records the user's visual references. No generated catalog music or artwork is presented as user content.

Recording identity is separate from tagged-file checksums. Migration 0007 retains stable recording UUIDs and immutable legacy anchors. Versioned FFmpeg f64 PCM evidence supports conservative audio matching, independent source metadata observations, and available-source metadata ownership. Exact checksum caches reuse completed inspections and retry transient failure. Legacy prepasses establish evidence before normal matching, including later-discovered witnesses after old locators disappear. Ambiguous historical IDs remain separate. All-changed legacy libraries cannot prove continuity and keep historical references unavailable rather than guessing by path. See docs/recording-identity.md.

The scanner owns a cap-std root directory handle for its complete scan. Files open relative to it, and FFmpeg reads a private input copy. Hashing is size/deadline/cancellation bounded with two reader permits; media tools have two permits, bounded output, error-diagnostic rejection, timeouts, and explicit child reaping. Directory walks stream without sorting entire folders, observe cancellation even through non-audio files, and cap directory depth at 64. Missing/displaced sources reconcile in 64-source pages. Indexes support bounded checksum and metadata fallback lookups.

Docker scripts/verify.sh passed 62 Rust tests, 30 frontend tests, formatting, Clippy with warnings denied, generated-contract checks, production build, zero audit vulnerabilities, and Chromium workflow. Final successful log: /tmp/hito-identity-verified.log. Ten container tag-rewrite fixtures, 32-bit precision, upgrade permutations, source replacement, private inputs, tool timeout/cancellation, large-file runtime shutdown, and directory-depth policy pass. Query-plan checks with actual migration schema and 100,000 duplicate locators avoid temporary sorting for checksum resolution, metadata fallback, and reconciliation. This does not prove release performance gates.

Foundation, UI/artwork, and identity received two independent review cycles each. The final identity cycle found additional issues, fixed with regression checks without a third review. The final identity privacy reviewer was source-only. Review evidence is .agents/meta/review-chain-report.md. Do not claim the final patches were independently certified clean.

Browser tests use their own fixtures/e2e-generated read-only mount and temporary database. The earlier shared fixture path included additional personal music and caused an expected-count failure; no music was removed. Browser artifacts under root/app test-results are ignored. Normal deployment must never be used for first-run tests.

## Deployment

Normal server http://localhost:8080 is healthy after migration 0007. It preserves volume hito_hito-data and legacy backend identity server. The stopped-volume archive backups/hito-before-recording-identity-20261015T151001Z.tar.gz preserves the prior database/WAL/sidecar. A separate read-only Docker comparison retained 1 account, 7 recordings, 1 playlist, 2 entries, 1 like, 6 progress rows, and the same sidecar. Health is ready and setup remains false. No authenticated production rescan was triggered; evidence adoption runs on the next ordinary scan. Never read credentials or session hashes to bypass authentication.

The older backup backups/hito-before-foundation-20261015T132403Z.tar.gz also remains. Backups directory is mode 0700 and archives are private. No signing credentials or provider accounts are configured.

Rust 1.99 is pinned for project/Docker/editor agreement; real rust-analyzer LSP expanded tracing::instrument. Graceful idle SIGINT/SIGTERM and Docker stop have regression evidence. Active long-stream shutdown and platform release gates remain unfinished.

## Next work approved by the user

Implement multiple music folders and their scan status/library access in administrator Settings. Normal listeners must not see administrative setup complexity.

The user specifically objected to needing HITO_MUSIC_FOLDER. The approved installation approach is a host-side native folder picker that configures read-only Docker mounts automatically; manual environment configuration is advanced only. A browser admin picker browses already server-accessible locations and cannot grant Docker new host directories. Desktop discovers the standard Music folder and uses a native picker for extras. These are approved requirements, not implemented features. Do not ask for approval again.

TODO.md is the actual remaining checklist. Full release work still includes artist/album pages, metadata providers/personal corrections, three desktop systems and native playback, durable offline replicas/import, explicit downloads, HTTP sources/relay/conversion, four debrid adapters, reference services, RSS/audiobooks, lyrics, the separate community service, performance/recovery/packaging/signing/restore/update acceptance. Most remain unimplemented.
