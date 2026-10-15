# Implementation status

The full release scope is in [release-plan.md](release-plan.md). This repository is a development build. The public release is not ready.

## Current quality work

The structural refactor, first substantial UI composition pass, tag-independent recording identity, multiple-folder Settings, and artist/album pages are implemented. Rust uses cohesive capability services with private SQLite storage, opaque authenticated actors, explicit backend device ownership, small HTTP adapters, and generated contracts. React uses focused pages and workflow hooks, a recording spotlight and artwork grid/list, grouped navigation, a listening sidebar, expanded player, and tabbed settings. Development setup launchers generate read-only Docker mounts from native folder selections. See [architecture decisions](architecture-decisions.md), [interface direction](../DESIGN.md), [folder behavior](music-folders.md), and the actual [TODO checklist](../TODO.md).

## Verified development build

| Area | Evidence | Limit |
| --- | --- | --- |
| Docker deployment | Isolated server passed the browser workflow using separate synthetic fixtures; normal persistent server rebuilt and healthy after migration 0012, with existing data verified against a stopped-volume backup | Local ARM64 Docker only |
| Identity and access | HTTP tests cover setup, sessions, administrator-created accounts, membership grants/revocation, private activity | No PIN locks or provider credential storage |
| Indexing and playback | Public fixtures cover tag-independent identity, exact checksums, source metadata ownership, replacement/restoration, conservative legacy witnesses across scan orders and moved roots, failed-decoder retry, and cache reuse | No acoustic/provider matching, personal metadata overrides, or incremental watcher |
| Artists and albums | Public tests cover exact credits, multiple release appearances, disc/track order, duplicate copies, repeated performances, Unicode/literal search, retags, scoped covers, paging, and permissions | Credits are not canonical people; provider matching, provenance APIs, overrides, and playback across all pages remain |
| Music folders | Public core/HTTP tests cover multiple folders, containment, administrator access, independent scans, removal/reconnection, host ownership, native filenames, canceled decoders, and expired-attempt fencing | Automatic desktop discovery and native desktop selection remain |
| Filesystem outages | Normal Linux root-open injection verifies deadlines and healthy-grant isolation; a separate libc-backend enumeration test verifies inactivity failure, preservation, subsequent healthy scans, shutdown, and continued slow progress | Enumeration injection does not exercise production Linux raw syscalls; real disconnected/network drives and other platforms remain unverified |
| Host setup | Two CLI tests verify installation adoption, stable grants, private files, rejected overlaps, and Windows paths; actual Compose config preserves literal dollar signs and custom projects; shell and PowerShell syntax checks pass | Native dialogs, permissions, drive reconnects, discovery ambiguity, backup/rollback behavior, and packaged installation need live platform acceptance |
| Artwork | HTTP cases cover embedded precedence, folder covers, private library permissions, symlink containment, FIFO avoidance, aggregate metadata limits, and compressed-frame fallback; additional core/HTTP regressions verify release-specific covers and no alternate-release fallback | Compressed ID3 parsing is disabled; general metadata and codec compatibility remain release work |
| Collection synchronization | Tests cover identical replay, conflicting ID reuse, stable entries, deletes winning over late edits | No durable desktop replica or offline outbox |
| Browser player | Tests cover source recovery, stale media errors/promises, short-track progress, persisted speed, and queue render isolation | No gapless or codec compatibility proof |
| Browser flow | Clean run passed two-folder connection/status, picker Escape/focus restoration, mobile layout, artist/album navigation with continuous playback, grid/list changes, expanded-player focus/errors, settings keyboard tabs, private playlists, and cross-tab account switching including delayed JavaScript | Chromium in Docker; other browser/platform acceptance remains |
| UI composition | Desktop and phone screenshots inspected for the library, expanded player, settings, folder picker, and artist/album pages; neutral missing-cover placeholders verified | Provider/download/desktop settings and final polish depend on unfinished capabilities |
| API and UI builds | Docker checks passed 92 Rust workspace tests, one separately run enumeration fault test, 36 frontend tests, two setup CLI tests, formatting, Clippy, generated contracts, production build, and audit with zero vulnerabilities | Development checks do not prove release gates |
| Legacy upgrades | Configuration and identity tests preserve historical references. Local migration 0012 retained account/recording/source IDs, file checksums, collections, progress, membership, and saved host identity against a stopped-volume backup; foreign keys pass | General backup restoration and platform acceptance remain |
| Process shutdown | CLI SIGINT/SIGTERM tests and isolated production Docker stop exit successfully with lifecycle logs | Active long jobs, long streams, and forced termination remain recovery gates |
| Logging | Trace capture rejects credential/private-input leaks while retaining request correlation and safe categories; browser allowlist tested | Future providers need separate redaction checks |
| Independent reviews | Foundation, UI/artwork, recording identity, folders, and artist/album browsing each received two review cycles; findings, fixes, and corrected evidence are recorded in the review report | Final-cycle fixes received regression checks without a third independent cycle; final identity/folder privacy reviews were source-only |
| Editor macros | Docker and project Rust 1.99 agree; real rust-analyzer LSP expanded tracing::instrument successfully | Existing editor processes must use the pinned toolchain |
| Query ordering | Actual SQLx SQLite plan on 100,000 synthetic rows uses catalog ordering index; migration-backed SQLite checks on 100,000 duplicate locators avoid sorting for checksum, metadata-owner, and reconciliation lookups; compilation-heavy Catalog API smoke test covers 100,000 synthetic tracks before/after ANALYZE | Not a playback or API latency benchmark |
| Media inspection | Ten container retag fixtures and 32-bit precision checks pass; timeout/cancellation releases children and temporary inputs; large-file cancellation permits prompt runtime shutdown; selected-root handle survives a pathname change | Linux FFmpeg evidence only; native Symphonia, gapless, broad decoder compatibility, and other platforms remain release gates |
| Plugin SDK | Manifest version/capability validation test | No installed plugins or HTTP provider client |

## Release work still required

- Live native setup acceptance and distributable packaging on all three systems. Multiple-folder Settings and development launchers exist; the browser still cannot grant new host mounts.

- Desktop local backend and Tauri clients for macOS, Windows, and Linux; Symphonia/CPAL/FFmpeg native playback and media controls.
- Durable offline client operations, server import preview, device-local availability across replicas, account switching under interrupted synchronization.
- Persistent downloads, shared file references, playlist synchronization, storage exhaustion and removal handling, separate playback cache.
- HTTP sources, source preferences/overrides, external relay, conversion, bitrate settings, debrid preparation and recovery.
- MusicBrainz enrichment, metadata provenance, personal edits, unlinking/splitting/undo and administrator promotion.
- Real-Debrid, AllDebrid, Premiumize, TorBox, music-folder/Jamendo/YouTube reference plugin deployments and live checks.
- Podcast RSS, audiobooks, chapters/bookmarks, playlist import/follow, timed lyrics and backing-track interfaces.
- Separately deployed opt-in community ranking/correction service, validation, moderation, rate limits and redacted logs.
- Full language-neutral HTTP plugin specification and provider data contracts. Application OpenAPI/client generation is implemented.
- Performance targets, codecs, gapless/recovery fixtures, outages, concurrent synchronization, download and permission gates.
- Container and binary distribution, signed installers, Linux packages, upgrades, approval-based updates, backup restoration, platform verification.
- Public community hosting and DNS, protected signing material, provider test accounts, Apple notarization and SignPath acceptance.

Do not infer release completion from a passing development test or a healthy local container.
