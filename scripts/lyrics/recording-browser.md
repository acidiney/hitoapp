# Historical packaged recording lyrics verification

This guide replays the historical v1 static-reading UI. Its browser spec and fixtures were removed from current HEAD when independent lyric identities replaced that UI. Retrieve the complete gate from the pinned commit below. It does not qualify the current UI or automatic lyric lookup.

The packaged reading and lifecycle cases passed in Chromium and WebKit against this exact runtime:

- Source: `ee17d7336c8942a6a283d3c59d635fa0f6c2bdfb`.
- Immutable image: `sha256:003cd13c5e575c3bfe77d084772f506fbe4edd32db6e6e16c5419775184e438c`.
- Product inputs: 1,970 committed files, SHA256 `b3240208bc22b403f2452afb1cd5ce532d5012bc031f3df05c2672d5af871999`.
- Four-case evidence: `qualified.json` SHA256 `ed7c9db8042a99d8670dcaff758a3080c24d6fbd16bddc1e3c20083ace4c8b0c`.
- Executed gate overlay: 11 files, SHA256 `71721f448ce5341db3a691514ce528f407a1a32800ccc77d01764a891cc24caa`. Only this guide changed afterward to record the result.

Use the exact historical runtime above, or a separately qualified matching checkpoint. This gate cannot qualify a newer application image. The operator supplies the matching identities:

```sh
export HITO_PACKAGED_IMAGE=sha256:<immutable-image-id>
export HITO_PACKAGED_SOURCE=<exact-source-commit>
export HITO_PACKAGED_INPUTS=<product-input-sha256>
python3 scripts/lyrics/recording-browser.py chromium webkit
```

The runner verifies the immutable image ID, revision and product-input labels, the exact source HEAD and committed product bytes, and the running container image. It records a separate manifest for the reviewed browser-gate overlay. It never accepts a changed HEAD as the source of an older artifact. To reproduce evidence after gate commits, use an isolated checkout of the qualified source and archive only the reviewed gate paths into it as an untracked overlay; the original committed product inputs must remain unchanged.

For the qualified runtime above, use those exact three identities. Create a fresh isolated checkout and copy only the eleven gate files from the immutable historical commit. Run these commands from a checkout containing its Git objects:

```sh
git worktree add --detach ../hito-lyric-reading-checkpoint ee17d7336c8942a6a283d3c59d635fa0f6c2bdfb
git archive 3bfb484d2bcc497b55998353540a7096feff684e \
  apps/web/e2e/recording-lyrics.spec.ts \
  apps/web/e2e/fixtures/recording-lyrics-evidence.ts \
  apps/web/e2e/fixtures/recording-lyrics-lifecycle.ts \
  apps/web/e2e/fixtures/recording-lyrics-provider.ts \
  apps/web/e2e/fixtures/recording-lyrics-state.ts \
  apps/web/e2e/fixtures/recording-lyrics-workflow.ts \
  scripts/lyrics/recording-browser.py \
  scripts/lyrics/recording-browser-verify.py \
  scripts/lyrics/recording-browser.tools.yaml \
  scripts/lyrics/recording-browser.compose.yaml \
  scripts/lyrics/recording-browser.md | tar -x -C ../hito-lyric-reading-checkpoint
cd ../hito-lyric-reading-checkpoint
```

Then set the three identities and run the verifier above. A newer committed HEAD requires its own matching runtime and acceptance workflow.

Every case creates a unique Compose project, a private tmpfs database and an unprivileged runtime with a read-only filesystem and read-only deterministic original WAV fixtures. The browser shares only that runtime's network namespace. The existing `hito-spotify-albums-tools_default` network and `hito_node-modules` volume are external prerequisites. No ports, normal server volumes, Docker socket, plugin downloads or package installations are used. The tools override is versioned in `recording-browser.tools.yaml`; there is no private `/tmp` Compose dependency.

The reading case uses the real setup UI, plugin declaration review and installation, Discover adoption and normal recording-details link. Only an explicit **Show lyrics** contacts the provider. The gate compares the exact retained observation, installation revision and opaque provider recording revision; checks escaped static text and credit links; and retries to a valid negative result. It preserves an unsaved metadata draft, full private catalog/playlist/activity/observation state, original WAV bytes and metadata, and checks that no persistent download exists. Playback, preparation, source refresh and implicit mutation traffic are forbidden during lyric reads. Browser console snapshots and operational logs are checked against raw and JSON-escaped private canaries, with exact generated correlation/outcome evidence.

A separate lifecycle case passed with two actual held provider responses per engine. Both provider bodies retired before the fixture released them, and immediate explicit retry succeeded after cancellation and navigation. The host completion events were exactly `cancelled`, `ready`, `cancelled`, `ready`, each with its own UUID correlation and timing. Reading produced exactly `ready`, `not_found`. The gate observed no audio, preparation, downloads, implicit writes, unexpected failed requests, JavaScript errors or private console values. Explicit download-status reads returned 404, and private state and original files were preserved. Source and gate hashes matched before and after all four cases.

Earlier packaged evidence exposed missing completion events when HTTP handlers were dropped. The completion owner now records those cancellations; a separate public regression also exposed and verified repair of span retirement under the captured dispatcher. Historical scoped logging failures recorded as samples `2260` and `27158` remain unexplained. Passing controls and this packaged result do not establish their cause or resolve all application logging requirements.

These results use owned protocol fixtures and Linux browser engines. Account-switch and late-owner-response browser controls, live provider compatibility, physical platforms, verified timed highlighting and backing-track playback remain separate gates. Static DOM lifecycle tests and host protocol tests provide separate evidence; this packaged gate does not qualify their entire scope. This workflow renders static lines and does not establish timing alignment or a completed public release.
