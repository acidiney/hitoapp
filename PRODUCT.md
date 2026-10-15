# Hito

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Listeners who own audio files or use several audio providers and want one library. Administrators operate a self-hosted server for independent accounts.

## Product purpose

Reliable playback across local files, HTTP streams, and external providers. The shared React interface runs in a browser and, later, a desktop application with its own local Rust backend.

## Capabilities and constraints

The first public release targets macOS, Windows, Linux, a self-hosted server, reference plugin services, and a community service. The application uses AGPL-3.0-only; the plugin SDK uses Apache-2.0. Mobile follows later. Full browser offline playback is outside this release.

Downloads require an explicit action. Device queues stay independent. Contributions require separate opt-ins for playback outcomes and metadata corrections, disabled by default. Third-party plugin code never runs inside Hito.

## Operating context

Development and the initial deployment run in Docker. SQLite stays on local persistent storage. Music folders mount read-only. Provider credentials, signing credentials, public DNS, and hosting are external dependencies.

Nontechnical users choose music folders through a file explorer. Desktop discovers the standard Music folder and uses the native folder picker for additional locations. Server installation needs a host-side launcher that selects folders and configures Docker read-only mounts automatically. Editing `.env` or setting `HITO_MUSIC_FOLDER` is an advanced option, not the required setup flow. Administrator folder management belongs inside Hito Settings. A browser can browse server-accessible folders but cannot grant new host filesystem mounts by itself. Guided installation and multiple-folder management remain unimplemented in the current development build.

## Evidence on hand

The approved first-public-release plan is recorded in docs/release-plan.md. There is no supplied music, branding, provider account, or performance evidence. Test audio must be identified as a fixture.

## Open decisions

The user supplied five music-app references and pinned a dark charcoal, orange-accented direction with cover-led content, grouped navigation, a persistent player, and clear settings. Production hosting and DNS remain unspecified.
