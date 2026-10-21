# VPS deployment

The development deployment at `https://hito.example.com` runs source revision `9279340057d102a56c398b20b457efdfcbb4bb40`, installed on 2026-10-20. It is available for testing, but real YouTube playback is currently failing. This installation does not satisfy the public release gates.

## Installed services

Four Linux amd64 images were built locally through Docker Compose, transferred with a verified SHA-256 checksum and verified natively on the VPS. They run with a read-only root filesystem, user `10001:10001`, dropped capabilities, `no-new-privileges`, resource limits and rotated logs.

| Service | Immutable image ID |
| --- | --- |
| Server | `sha256:e5ab7e579041b7d824696a2dd100eafae7fd476114d2985b5922e96573d2313b` |
| Folder | `sha256:f27253d36e02b92da4704225c3afc4b64cb0657b7ff61d9bf3a405bc7a447e98` |
| Spotify | `sha256:dc1f98f07056a876b8c0398eca8e3d4508d491f0de501ec45279395b88569a08` |
| YouTube | `sha256:bec76e23d602cbce49dc60474e3ee0c9b33635ecbbbdb315f127a698a9cb3e9c` |

Only the server publishes a port, on host loopback. The existing host Caddy configuration adds the Hito hostname and proxies to that listener. The three plugins remain on the private Docker network. An independent administrator account and three personal plugin connections were created on the VPS. No local account database or provider credentials were copied.

The folder service supplies three generated WAV signals for testing. Spotify provides public playlist metadata and artwork, not Spotify audio. The YouTube image bundles yt-dlp `2026.08.19` and Deno `2.9.7`. Real-Debrid is not installed in this deployment; its current curated-WAV service still needs separate private configuration and application qualification.

## Verified behavior

An isolated Chromium workflow completed against the public HTTPS origin in 5.06 seconds. It verified login, a Secure/HttpOnly/SameSite Strict session cookie, the three plugin connections, folder search, decoded generated audio, uninterrupted playback during navigation, pause and logout. It observed no JavaScript errors or foreign browser requests. This proves the public application and its folder relay, not real external music playback.

A separate live Spotify read returned 411 playlist entries and retained verified album information in the sampled entries. YouTube search returned candidates, but source extraction failed before audio became available. The deployed core incorrectly settles these provider failures as `no_suitable_source`. Verified source now retains a separate `provider_unavailable` outcome and shows it in the browser without duplicate resolution requests. Its directed tests and independent repeats pass, but this correction and migration 0055 are not deployed. It improves failure reporting and retry behavior; it does not make blocked audio available. See [source verification and remaining acceptance](automatic-sources.md#provider-failure-outcomes).

Native probes with the installed extractor exited unsuccessfully, with stderr mentioning sign-in and bot confirmation on the existing VPS connection. The `web_embedded` and `tv_downgraded` clients returned the same failure. IPv6 did not produce playable metadata. No cookies, personal browser session, proxy or Mac-hosted service were introduced.

An isolated candidate then replaced only the complete extractor bundle with official nightly `2026.10.10.233141`, using its published archive checksum. The Rust plugin and Deno binaries remained byte-identical. One explicit `tv_samsung` metadata probe on the same Docker bridge and IPv4 connection failed in 2.89 seconds, without a metadata object and with exactly the same error-output hash as the earlier failures. No audio was requested. Temporary containers and staging files were removed; the candidate image was retained for inspection and was not deployed. This version upgrade did not resolve playback.

Those receipts summarize stderr, not the actual player responses. Their zero format counts are fallback values when no metadata object is returned, so the advertised format counts remain unknown. They do not establish a `LOGIN_REQUIRED` player status, an IP ban, successful challenge-solver loading or failure of Hito's AAC selector. Diagnosis still needs the selected recording's player status, streaming-data counts and observed solver loading. If usable player data arrives, the extracted formats before selection must also be checked. Automatic VPS playback remains unresolved.

## Backups and remaining acceptance

Both complete persistent volumes were archived while their owning services were stopped. The services restarted healthy, with a measured stop/backup/start interval of 7.54 seconds. Archive hashes and contents were checked again after copying them off the VPS, together with private configuration. The backup retains SQLite state, host identity and the credential key.

On 2026-10-21, those off-host archives passed restoration into disposable local volumes. Extraction verified the exact entries, file bytes and metadata before SQLite opened either copy, retaining WAL and SHM files. The audit compared schemas, typed values, row identifiers and all 54 migration checksums, and verified database integrity, foreign keys, the credential key, host identity and private configuration. The matching deployed CLI completed a 54-to-54 migration check with no migrations applied. All logical comparisons passed again afterward; the original backups were unchanged and disposable resources were removed. No encrypted credential rows were present, so this does not test credential decryption.

Before this rehearsal, 14 parser cases, 13 Docker cleanup/interruption controls and six real-process controls passed, followed by an independent 33-case repeat. Review findings about process ownership and interruption at cleanup entry were reproduced and corrected. The real preservation rehearsal completed in 4.85 seconds. These timings describe verification runs, not recovery-time targets.

A subsequent matching-version server checkpoint passed on a fresh disposable restoration in 9.28 seconds. Before startup, all fourteen worker-eligibility predicates were empty, and the host identity and storage binding matched. No jobs were cleared or rescheduled. The exact deployed server ran without network access or published ports, under its unprivileged container policy. Seven anonymous requests verified health, completed setup, rejected account/player access and the login-only Inertia page. SIGTERM joined with exit zero, without an out-of-memory event or forced kill.

After shutdown, all 121 application tables and 7 folder tables matched their prior schemas, typed rows and row identifiers, including 7,025 application rows and 38 folder rows. All 54 migration checksums, keys, host identity and private configuration remained exact. Original archives and configuration, and the normal Mac server's identity, image and start time were unchanged. Cleanup left no owned containers or volumes. This does not test authenticated access, credential decryption, folder-worker startup, restored provider connections or an upgrade.

The startup operator also passed 35 parser, process and ownership controls. These include refusing a replacement container with a different full ID and handling Docker metadata above the bounded runner's output limit through closed field projections. An independent repeat of the large-metadata control and the real-backup startup both pass. Earlier failing attempts remain recorded separately; the final projection change received root source review and actual regression verification.

Real external playback, recovery, authenticated and folder-worker restoration, upgrades, account/load acceptance and platform release gates remain open. The normal Mac installation was kept separate and unchanged. Installation and recovery instructions are in [the VPS runbook](../deploy/vps/README.md); the full scope remains in [the release plan](release-plan.md).
