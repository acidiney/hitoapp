# Historical packaged lyric provider chooser

This replays a historical v1 chooser checkpoint. Its browser files and npm script were removed from the current application when independent lyric identities replaced that UI. Use the pinned gate commit below; do not archive the removed paths from current HEAD. This runner does not qualify the current lyric UI or the automatic-read backend.

The single tracer passed in Chromium and WebKit against the source checkpoint below, including an independent two-engine repeat. Runtime isolation, exact provider selection, state preservation, safe completion logs, final source/module pins and cleanup pass. The existing four static reading/lifecycle cases remain separate and unchanged. This does not qualify live providers or the full release.

The runner requires the new product source `6cc3a0a42fcbde7bb590bcd0de644a5aa8cefa23`, its immutable runtime image and committed product-input digest. The earlier source-396 image lacks the chooser and is rejected by the source/image contract.

The qualified image is `sha256:d1622a90f01be763b6391f241df935e2f98bac124400ce1ea0331509c854e024`. Its 1,994 product inputs hash to `bff5049f4248acd9a3a495d43adf752cc0f7c08377933b2543ad2ec04c68342b`. This qualifies the frozen checkpoint. Later application changes need their own acceptance run.

Run from a detached checkout of that product commit with only the reviewed gate overlaid from the current tree. Keep those new gate files untracked in the checkpoint; the source pin intentionally reads its committed product tree. From a checkout containing the historical Git objects:

```sh
git worktree add --detach ../hito-lyric-choice-checkpoint 6cc3a0a42fcbde7bb590bcd0de644a5aa8cefa23
git archive 26d1d31d2880b4a3a60eb41d31bc211f66b71249 \
  apps/web/e2e/recording-lyric-provider-choice.spec.ts \
  apps/web/e2e/fixtures/recording-lyric-provider-choice-evidence.ts \
  apps/web/e2e/fixtures/recording-lyric-provider-choice-provider.ts \
  apps/web/e2e/fixtures/recording-lyric-provider-choice-state.ts \
  apps/web/playwright.lyric-provider-choice.config.ts \
  scripts/lyrics/provider-choice | tar -x -C ../hito-lyric-choice-checkpoint
cd ../hito-lyric-choice-checkpoint
```

```sh
export HITO_PACKAGED_IMAGE=sha256:d1622a90f01be763b6391f241df935e2f98bac124400ce1ea0331509c854e024
export HITO_PACKAGED_SOURCE=6cc3a0a42fcbde7bb590bcd0de644a5aa8cefa23
export HITO_PACKAGED_INPUTS=bff5049f4248acd9a3a495d43adf752cc0f7c08377933b2543ad2ec04c68342b
python3 scripts/lyrics/provider-choice/run.py chromium webkit
```

The tracer creates a fresh administrator through the public setup API, seeds deterministic local originals and private owner/peer state, and personally reviews and connects two owned HTTP services. It adopts A through Discover and explicitly calls the public generic source resolver to attach B. It validates the retained observations through public metadata before arming the chooser assertions. It never writes SQLite or invents an observation.

Choose must return exactly A and current B without provider work. Selecting B must not request lyrics. A separate Show must send B's exact observation and installation revision, return B's opaque reference, and render its escaped plain lines and structured credits. The primary metadata, unsaved editor node and draft, complete owner/peer snapshots and original WAV metadata/bytes must remain unchanged. The tracer forbids audio, preparation, downloads and implicit browser mutations during the chooser workflow. Read-only diagnostic snapshots use the existing public saved-progress query, which is POST by API design.

The app uses `network_mode: none`. The nonroot, read-only browser and both loopback provider fixtures share only its network namespace. There are no published ports, bridge networks, normal data volumes, external services, Docker socket mounts or package installations. Every run has a unique project and private bounded tmpfs database; cleanup names only that project and its exact browser container. Runtime image/source labels, mount and namespace contracts, the old 11 gate files, committed product inputs and the read-only Playwright module bytes are checked and frozen.

Expected optional artwork/download 404s must belong to an owned recording, use exact GET routes and canonical bounded JSON with no-store and UUID correlation. Native console errors must match those exact response URLs and status messages one-to-one. This tracer performs no logout and permits no 401. Operational logs and console snapshots exclude raw and JSON-escaped private canaries. Safe summaries retain numerical checkpoints, request counts, workflow outcomes, durations and generated correlations. Screenshots, traces and video are disabled.

Source-only qualification uses the tools profile in `tools.yaml`. The node service has 512 MiB; the types service has 1.5 GiB and a 1 GiB heap. Neither can access a network. Both bind the existing dependency volume read-only. A successful test collection or TypeScript check does not establish any browser or runtime result.
