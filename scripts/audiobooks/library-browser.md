# Packaged audiobook library browser gate

Run from this checkout with the final shared runtime image already built. The runner never builds an image or runs Cargo.

```sh
HITO_PACKAGED_IMAGE=sha256:<immutable-image-id> \
HITO_PACKAGED_SOURCE=<40-character-source-commit> \
HITO_PACKAGED_INPUTS=<64-character-product-input-digest> \
python3 scripts/audiobooks/library-browser.py chromium webkit
```

The source fingerprint helper is `scripts/artwork/source-pin.py`. Before the two browser lanes are integrated, set `HITO_PACKAGED_PIN_HELPER` to that helper in the artwork checkout. The existing `hito_node-modules` volume, Node 22 and Playwright 1.56.1 images, and `hito-spotify-albums-tools_default` network must be available. The owned versioned `scripts/audiobooks/library-browser.tools.yaml` override mounts modules read-only and declares that network external. No temporary Compose file is required, no dependencies are installed, and the runner neither builds nor pulls tools images.

Each engine runs two functional cases and one privacy boundary selftest against separate fresh servers. They create the administrator through the UI, index two owned WAV originals through the folder picker, and seed 121 grouped books through public synchronization. Equal names make UUID ordering observable. A peer account, deleted book, ungrouped playlist, personal metadata, duplicate playlist occurrences, likes, and progress provide preservation controls.

The traversal case checks pages of 60, 60, and one book, Previous and First links, reload, Back and Forward, and a document token that distinguishes an Inertia visit from a full reload. The grouping selector must exclude grouped books on every page. Two additional books with currently valid control-character names exercise Next and Previous cursors larger than 2 KiB through the same packaged HTTP API.

The recovery case drops one real accepted Inertia response and holds another until the actual ten-second deadline. Retry must retain the exact page address. Renaming and deleting the current cursor anchor through authorized public synchronization must produce real HTTP 409 responses and usable First links. It then signs out and signs into the peer account while a real owner response is held. Releasing that response must leave only the peer's book visible.

All navigation must preserve independent account snapshots, original audio bytes and file metadata, and explicit HTTP 404 download-status responses. The transport guard rejects audio, source work, downloads, extra synchronization, foreign requests, and unexpected reads or writes. Session cancellation exceptions require an explicit account-switch phase and already observed accepted login/logout responses. Browser request and public fixture request budgets are 250 and 180 respectively. The reused initial playlist helper makes one additional declared seed synchronization request outside the latter counter.

The runner checks the source fingerprint before and after, exact image ID and labels, the running container's image ID, read-only owned originals, an unprivileged read-only root, and tmpfs database storage. It publishes no ports and does not use normal application volumes. Evidence is saved beneath a private temporary directory. Canary values cover credentials, cookies, titles, paths, and cursors, including JSON escaping. Application console text, argument snapshots, and structured server logs must pass validation. The runner removes only its uniquely named containers.

Browsers expose request URLs in their native network diagnostics and developer tools. The gate counts these separately only for zero-argument native entries tied to the exact owned intentionally failed request and its observed response. WebKit also emits an empty-location `Web Inspector blocked <request> from loading` message; the full embedded URL must equal the registered request. No private text or URL is copied into diagnostic evidence. All JavaScript console calls remain under the strict canary check. A separate selftest uses real browser calls to prove that a native-looking JavaScript `console.error` and an unowned blocked request still fail, while the exact owned native event is classified separately. Its three contexts each have a 20-request bound and remain inside the combined 250-request budget.

An owned request's exception expires when another request uses its URL. The real transport selftest also sends a second unregistered request to the formerly owned URL and requires both an unknown transport failure and a private-console rejection.

Current author qualification uses frozen product commit `84fd31da27f2d0d697575e389ef9c1bb302e5c9e`, runtime image `sha256:1a72e10cd1b18bd5859c8c0295f139650a5c74c4482e272d964133b95b247f7d`, and product fingerprint `2b56b8519f51218906bf55a6bb002fc2ad9289557094517b31d01b35fd0868fe`. The sixteen new gate files are an untracked fixture overlay during that qualification, including the owned tools override. Evidence includes their exact file hashes separately from the 1,864 unchanged committed product inputs. Review and qualifier commits identify the overlay when it is integrated.

The source helper deliberately requires the exact product HEAD and fingerprint. Committing the gate files creates a new HEAD and changes the tracked build-input set. To reproduce the current artifact, use an isolated checkout of the frozen product commit and extract only the new gate paths from the reviewed qualifier commits as an untracked overlay. To qualify a later committed checkout, build a new matching runtime image and supply its new source and fingerprint. Do not reuse an older image under a new source identity.

This gate does not qualify a physical Safari device, desktop packaging, provider playback, or a public release. The existing first-1,000-playlist grouping selector window remains a separate limit; this fixture does not claim to remove it. Query-work measurements and core/HTTP edge cases belong to the separately qualified source slice.
