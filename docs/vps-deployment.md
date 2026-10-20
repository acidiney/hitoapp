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

A separate live Spotify read returned 411 playlist entries and retained verified album information in the sampled entries. YouTube search returned candidates, but source extraction failed before audio became available. Current provider failures are also incorrectly settled by core as `no_suitable_source`; a new public-capability regression reproduces that classification error while preserving private state and exact receipt replay. Its correction is not deployed.

Native probes with the installed extractor returned the YouTube sign-in/bot challenge on the existing VPS connection. The `web_embedded` and `tv_downgraded` clients returned the same failure. IPv6 did not produce playable metadata. No cookies, personal browser session, proxy or Mac-hosted service were introduced.

An isolated candidate then replaced only the complete extractor bundle with official nightly `2026.10.10.233141`, using its published archive checksum. The Rust plugin and Deno binaries remained byte-identical. One explicit `tv_samsung` metadata probe on the same Docker bridge and IPv4 connection failed in 2.89 seconds, with no formats and exactly the same error-output hash as the earlier challenges. No audio was requested. Temporary containers and staging files were removed; the candidate image was retained for inspection and was not deployed. This version upgrade did not resolve playback.

## Backups and remaining acceptance

Both complete persistent volumes were archived while their owning services were stopped. The services restarted healthy, with a measured stop/backup/start interval of 7.54 seconds. Archive hashes and contents were checked again after copying them off the VPS, together with private configuration. The backup retains SQLite state, host identity and the credential key. Restoration of this deployment has not been verified.

Real external playback, recovery, backup restoration, upgrades, account/load acceptance and platform release gates remain open. The normal Mac installation was kept separate and unchanged. Installation and recovery instructions are in [the VPS runbook](../deploy/vps/README.md); the full scope remains in [the release plan](release-plan.md).
