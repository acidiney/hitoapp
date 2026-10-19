# Packaged collection artwork browser gate

Run from this checkout with the already built runtime image and existing `hito_node-modules` volume. The runner builds no image, runs no Cargo, and installs no dependencies.

```sh
HITO_PACKAGED_IMAGE=sha256:<immutable-image-id> \
HITO_PACKAGED_SOURCE=<40-character-source-commit> \
HITO_PACKAGED_INPUTS=<64-character-product-input-digest> \
scripts/artwork/run-packaged.sh chromium
```

Use `webkit` for the second engine. `compose.tools.yaml` defines the Node tools service without networking or temporary configuration files. The browser and server reuse the existing `hito-spotify-albums-tools_default` network. Each engine gets a fresh unprivileged server with a private tmpfs database, read-only synthetic music, no published ports, and no application volume. Only the runner's owned containers are removed.

The gate checks exact distinct recording, release, and playlist image bytes through the packaged server and real UI. Two independent editable copies retain duplicate entries, personal names and metadata, likes, and progress. Album and playlist navigation, private peer denial, provider revocation, logout, and retained-image retry must preserve the original audio/image files and the explicitly qualified collection/event state. The only final additions are the two deliberate cancellation-control imports.

Authenticated download-status reads must return exact private, no-store HTTP 404 responses. Three reads cover the original recording and two copies before retrieval. Seven final reads cover all three recordings and four copies, including both cancellation-control imports. These ten public fixture reads are reported separately from browser requests and artwork admissions. No audio, download mutation, source mutation, or implicit synchronization is permitted during artwork workflows.

Both engines check ordinary cover rendering and retained-image recovery after disconnect. Chromium additionally uses controlled layout to prove that a later successful real Release row leaves the failed hero's Retry visible. WebKit checks ordinary-layout coalesced failure followed by explicit Retry recovering both real Release images. Its result does not establish a distinct later-success ordering before Retry.

An earlier author run passed the durable download and final collection guards but failed the client-ready log assertion. Two diagnostic runs recorded real Albums-card cancellation followed by continuation on the hero. The gate now waits for the exact accepted Release job's client-ready event while its real card remains mounted before navigation. The diagnostics did not reproduce the exact historical failure sequence; no product defect was established. Bounded receipt/completion observations remain in the evidence.

Qualification uses frozen product commit `84fd31da27f2d0d697575e389ef9c1bb302e5c9e`, immutable image `sha256:1a72e10cd1b18bd5859c8c0295f139650a5c74c4482e272d964133b95b247f7d`, and product fingerprint `2b56b8519f51218906bf55a6bb002fc2ad9289557094517b31d01b35fd0868fe`. The reviewed gate is an untracked fixture overlay over those 1,864 unchanged committed product inputs. Its file hashes are recorded separately.

The source check requires that exact product HEAD and fingerprint. Committing the gate creates a different HEAD and tracked build-input set. Reproduce the qualified image with an isolated checkout of the frozen product commit plus the reviewed gate paths as an untracked overlay, or build a new image matching a later checkout. Do not present the old image as a qualification of a new source identity.

This gate does not qualify followed-playlist artwork, a live provider account, physical macOS/Safari, desktop packaging, or a public release.
