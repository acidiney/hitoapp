# Implementation status

The full release scope is in [release-plan.md](release-plan.md). This repository is a development build. The public release is not ready.

## Current quality work

The structural refactor, first substantial UI composition pass, and tag-independent recording identity are implemented. Rust uses cohesive capability services with private SQLite storage, opaque authenticated actors, explicit backend device ownership, small HTTP adapters, and generated contracts. React uses focused pages and workflow hooks, a recording spotlight and artwork grid/list, grouped navigation, a listening sidebar, expanded player, and tabbed settings. See [architecture decisions](architecture-decisions.md), [interface direction](../DESIGN.md), and the actual [TODO checklist](../TODO.md).

## Verified development build

| Area | Evidence | Limit |
| --- | --- | --- |
| Docker deployment | Isolated server passed the browser workflow using separate synthetic fixtures; normal persistent server rebuilt and healthy after migration 0007, with existing data verified against a stopped-volume backup | Local ARM64 Docker only |
| Identity and access | HTTP tests cover setup, sessions, administrator-created accounts, membership grants/revocation, private activity | No PIN locks or provider credential storage |
| Indexing and playback | Public fixtures cover tag-independent identity, exact checksums, source metadata ownership, replacement/restoration, conservative legacy witnesses across scan orders and moved roots, failed-decoder retry, and cache reuse | No acoustic/provider matching, personal metadata overrides, or incremental watcher |
| Artwork | Six HTTP cases cover embedded precedence, folder covers, private library permissions, symlink containment, FIFO avoidance, aggregate metadata limits, and compressed-frame fallback | Compressed ID3 parsing is disabled; general metadata and codec compatibility remain release work |
| Collection synchronization | Tests cover identical replay, conflicting ID reuse, stable entries, deletes winning over late edits | No durable desktop replica or offline outbox |
| Browser player | Tests cover source recovery, stale media errors/promises, short-track progress, persisted speed, and queue render isolation | No gapless or codec compatibility proof |
| Browser flow | Clean run passed setup, scan, playback across navigation, grid/list changes, modal Close/Escape and queue-empty focus fallback, visible playback failures, keyboard settings tabs, private playlists, and cross-tab account switching including delayed JavaScript | Chromium in Docker; other browser/platform acceptance remains |
| UI composition | Desktop and phone screenshots inspected for the library, expanded player, and settings; neutral missing-cover placeholders verified | Artist/album pages, provider/download/desktop settings, and final polish depend on unfinished capabilities |
| API and UI builds | Docker checks passed 62 Rust tests, 30 TypeScript tests, formatting, Clippy, generated-contract checks, production build, and audit with zero vulnerabilities | Development checks do not prove release gates |
| Legacy upgrades | Configuration tests preserve backend ownership; identity upgrade tests preserve historical references. Local migration 0007 retained account, recording, collection, and progress row counts and the saved host identity against a stopped-volume backup | General backup restoration and platform acceptance remain |
| Process shutdown | CLI SIGINT/SIGTERM tests and isolated production Docker stop exit successfully with lifecycle logs | Active long jobs, long streams, and forced termination remain recovery gates |
| Logging | Trace capture rejects credential/private-input leaks while retaining request correlation and safe categories; browser allowlist tested | Future providers need separate redaction checks |
| Independent reviews | Foundation, UI/artwork, and recording identity each received two review cycles; findings and fixes are recorded in the review report | Final-cycle fixes received regression checks without a third independent cycle; final identity privacy review was source-only |
| Editor macros | Docker and project Rust 1.99 agree; real rust-analyzer LSP expanded tracing::instrument successfully | Existing editor processes must use the pinned toolchain |
| Query ordering | Actual SQLx SQLite plan on 100,000 synthetic rows uses catalog ordering index; migration-backed SQLite checks on 100,000 duplicate locators avoid sorting for checksum, metadata-owner, and reconciliation lookups | Not a playback or API latency benchmark |
| Media inspection | Ten container retag fixtures and 32-bit precision checks pass; timeout/cancellation releases children and temporary inputs; large-file cancellation permits prompt runtime shutdown; selected-root handle survives a pathname change | Linux FFmpeg evidence only; native Symphonia, gapless, broad decoder compatibility, and other platforms remain release gates |
| Plugin SDK | Manifest version/capability validation test | No installed plugins or HTTP provider client |

## Release work still required

- Multiple-folder Settings and guided host installation with native folder selection and automatic read-only Docker mounts. The browser cannot grant new host mounts; environment configuration is an advanced option.

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
