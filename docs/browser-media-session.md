# Browser Now Playing integration

The browser player publishes the current authorized title, artist, verified artwork, duration, position and playback state through the browser's Media Session API. This is integrated in development source and passes isolated Chromium/WebKit acceptance. It is not yet deployed to the normal local server. Album metadata is not in the current player track contract; native operating-system integration and physical device acceptance remain open.

## Ownership

```mermaid
flowchart LR
  Queue[Account-owned browser player] --> State[Queue and playback state]
  Queue --> Position[Independent position subscription]
  State --> Session[Browser Media Session owner]
  Position --> Session
  Artwork[Current authorized artwork endpoint] --> Probe[One bounded decoded image probe]
  Probe --> Session
  Session --> System[Browser metadata and supported controls]
  System --> Commands[Explicit player commands]
  Commands --> Queue
  Close[Account teardown or queue completion] --> Clear[Clear metadata, position and handlers]
  Clear --> System
```

Position updates publish imperatively and do not rerender catalog pages. The bridge registers supported Play, Pause, seeking, Previous, Next and Stop actions. Play and Pause use explicit commands against the media element's actual paused state, including a pending Play promise. The shared native command adapter also retains explicit Resume/Pause direction; this does not add native OS media controls.

Artwork uses one ten-second probe of the current recording's same-origin authenticated endpoint. Only successfully decoded dimensions are published. The bridge does not start provider artwork retrieval or Download. An existing approved artwork workflow can notify it when a cover becomes ready. Recording/account changes cancel old probes and fence late callbacks.

Account teardown and queue completion clear metadata, playback state, position and registered handlers. Handlers retain an occurrence generation and cannot control a later account or queue occurrence. Missing browser APIs degrade independently without preventing ordinary playback. Display values stay out of workflow logs.

## Verification and limits

The integrated source passes all 273 frontend cases across 88 files, both TypeScript checks and its production build. Directed cases cover partial API support, repeated system Play before the native play event, seeking, track changes, queue completion, Strict Mode, account replacement, stale handlers, invalid positions and late artwork callbacks.

The prototype image `sha256:3c2941e1ea009813503c39cd0fb0ca1bfefea5477189d4202c6ae1be5278221c` combines the already verified migration-0045 server with these assets. Fresh Chromium and WebKit workflows pass in 5.6 and 5.7 seconds. They use native Media Session APIs, two owned WAV sources and an authorized decoded cover. They verify metadata and position, Play/Pause/Seek/Next/Previous/Stop, actual advancing audio bound to the expected source after track changes, remembered callback rejection and logout cleanup in the original document. A subsequent artwork request returns 401. No implicit Download, source choice, automatic resolution, external browser request or private console value is accepted.

The test forwards instrumented calls to native APIs before observing them. It invokes the registered system handlers from trusted browser clicks; it does not exercise physical media keys or lock-screen UI. Unsupported engines skip explicitly. Both tested engines expose the required APIs and pass without a skip or autoplay override.

Two test defects were corrected without application changes. The first assertion observed the paused property before the browser delivered its queued pause event. The test now waits within its existing eight-second limit for the same expected Media Session state. This follows the [HTML pause steps](https://html.spec.whatwg.org/multipage/media.html#internal-pause-steps). The logout gate initially tried to evaluate a document during navigation; it now observes actual native clearing in that original document, invokes remembered callbacks there and delivers only bounded boolean evidence. Independent gate review also required exact source identity after Next/Previous. Both final review findings are resolved; production review passes separately.

Chromium and WebKit backend checks examine 2,213 and 2,252 JSON records respectively without the fixed private fixture canaries. Both disposable servers stop with exit zero and no OOM. These counts describe these fixtures, not a complete privacy or reliability audit.

Physical Safari/phone presentation, OS artwork fetching, hardware controls, album publication and native desktop integration remain unverified or unfinished. This feature does not resolve the reported mobile Safari duration/silent-tail bug or establish gapless playback and release latency targets.
