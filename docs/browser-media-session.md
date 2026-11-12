# Browser Now Playing integration

The browser player publishes the current authorized title, artist, verified artwork, duration, position and playback state through the browser's Media Session API. This is deployed to the normal local development server after isolated Chromium/WebKit acceptance and stopped clone and normal preservation audits. The title, artist and album come from the queued recording. A recording with no album publishes an empty album. Native operating-system integration and physical device acceptance remain open.

## Ownership

```mermaid
flowchart LR
  Queue[Account-owned browser player] --> State[Queue and playback state]
  Queue --> Position[Independent position subscription]
  State --> Session[Browser Media Session owner]
  Position --> Session
  Artwork[Release then recording artwork endpoints] --> Probe[One bounded decoded image probe]
  Probe -->|image error| Retrieval[Shared artwork retrieval]
  Retrieval -->|ready revision| Probe
  Probe --> Session
  Session --> System[Browser metadata and supported controls]
  System --> Commands[Explicit player commands]
  Commands --> Queue
  Close[Account teardown or queue completion] --> Clear[Clear metadata, position and handlers]
  Clear --> System
```

Position updates publish imperatively and do not rerender catalog pages. The bridge registers supported Play, Pause, seeking, Previous, Next and Stop actions. Play and Pause use explicit commands against the media element's actual paused state, including a pending Play promise. The shared native command adapter also retains explicit Resume/Pause direction; this does not add native OS media controls.

Artwork probes the current track's same-origin authenticated images in the order the track rows use: the release image when the track has a release, then the recording image. Each probe has a ten-second limit, and only successfully decoded dimensions are published. A probe that fails with an image error asks the shared account artwork retrieval to fetch that cover, the same availability, fetch and status workflow the page covers use. The bridge adds no second fetch path and inherits its two-request, 16-workflow and two-minute bounds and its cached `unavailable` result, so an unavailable cover is checked once. When retrieval publishes a ready revision, the bridge probes again and updates the metadata. A timeout does not start retrieval. Track, recording and account changes cancel old probes and retrieval observers, and late callbacks are fenced. The bridge never puts provider URLs or private paths into metadata or logs. This behavior is in tested source only and is not deployed.

Account teardown and queue completion clear metadata, playback state, position and registered handlers. Handlers retain an occurrence generation and cannot control a later account or queue occurrence. Missing browser APIs degrade independently without preventing ordinary playback. Display values stay out of workflow logs.

## Verification and limits

The integrated source passes all 273 frontend cases across 88 files, both TypeScript checks and its production build. Directed cases cover partial API support, repeated system Play before the native play event, seeking, track changes, queue completion, Strict Mode, account replacement, stale handlers, invalid positions and late artwork callbacks.

The locally deployed image `sha256:3c2941e1ea009813503c39cd0fb0ca1bfefea5477189d4202c6ae1be5278221c` combines the verified migration-0045 server from application source `83ae186` with shared UI source `0e2a516`. Its Rust application source is unchanged from the prior deployment, and the integrated assets match the tested prototype byte for byte. Fresh Chromium and WebKit workflows pass in 5.6 and 5.7 seconds. They use native Media Session APIs, two owned WAV sources and an authorized decoded cover. They verify metadata and position, Play/Pause/Seek/Next/Previous/Stop, actual advancing audio bound to the expected source after track changes, remembered callback rejection and logout cleanup in the original document. A subsequent artwork request returns 401. No implicit Download, source choice, automatic resolution, external browser request or private console value is accepted.

The test forwards instrumented calls to native APIs before observing them. It invokes the registered system handlers from trusted browser clicks; it does not exercise physical media keys or lock-screen UI. Unsupported engines skip explicitly. Both tested engines expose the required APIs and pass without a skip or autoplay override.

Two test defects were corrected without application changes. The first assertion observed the paused property before the browser delivered its queued pause event. The test now waits within its existing eight-second limit for the same expected Media Session state. This follows the [HTML pause steps](https://html.spec.whatwg.org/multipage/media.html#internal-pause-steps). The logout gate initially tried to evaluate a document during navigation; it now observes actual native clearing in that original document, invokes remembered callbacks there and delivers only bounded boolean evidence. Independent gate review also required exact source identity after Next/Previous. Both final review findings are resolved; production review passes separately.

Chromium and WebKit backend checks examine 2,213 and 2,252 JSON records respectively without the fixed private fixture canaries. Both disposable servers stop with exit zero and no OOM. These counts describe these fixtures, not a complete privacy or reliability audit.

The same deployed image also passes fresh Chromium/WebKit next-entry workflows after integration, preserving background buffering and natural advancement. Local publication retains all nine ordered server configuration files, the prior image alias, fresh backups and unchanged provider services. Health, exact OpenAPI/assets, anonymous player rejection and loopback/LAN checks pass without authenticating a normal listener.

Physical Safari/phone presentation, OS artwork fetching, hardware controls and native desktop integration remain unverified or unfinished. In native playback mode (`native_playback`) the web app owns no audio element and publishes nothing to the Media Session. The operating-system panel there depends on the Rust desktop host, which has no Now Playing code yet. This feature does not resolve the reported mobile Safari duration/silent-tail bug or establish gapless playback and release latency targets.

## Duration revisions in verified source

The browser player now observes `durationchange` as well as initial metadata. When the browser revises an estimate, the timeline and position subscribers receive the new duration without changing the selected recording, source, queue, speed or heard progress. Nonfinite or negative durations become unknown. The current source, media element and playback generation fence stale events after source changes, prepared-media handoff and account teardown. Repeated identical values do not notify subscribers. A safe debug event records the recording identifier, outcome and handler timing.

Six deterministic public-player cases and 64 related playback regressions pass in Docker, including an independent repeat, both TypeScript checks and formatting. Fresh source review found no unresolved issue. The first regression reproduces a browser estimate changing from 440 to 220 seconds without another metadata event. This qualifies the observation behavior in source only. It is not deployed and does not establish the cause or resolution of the reported physical mobile Safari duration and silent-tail bug.

## Overlong browser duration estimates

Safari can estimate `media.duration` far above the real length, for example for variable-bitrate MP3 without a Xing header, large tags or mis-signalled HE-AAC. The timeline then runs about twice as long, audio stops in the middle of it, and the rest is silent. This is the suspected cause of the reported 3:40 song. It has not been reproduced on a physical device.

The player now reconciles the browser duration with the recording's known `duration_ms` on `loadedmetadata`, `durationchange` and prepared-media handoff. The known duration wins only when the browser value exceeds it by more than 10 percent and by more than 5 seconds. Provider metadata is imprecise by a few seconds, so smaller differences keep the browser value. A shorter browser value is a measurement and always wins. A recording with no known duration (`duration_ms` of 0) always uses the browser value. The chosen value drives the timeline, the seek clamp, Media Session position state and progress. Original bytes are never rewritten. A debug event `playback.duration_server_preferred` records the recording identifier, the server duration (`server_duration_ms`) and the browser estimate (`media_duration_ms`). It never records titles or paths.

When the known duration won, the browser never fires a trustworthy `ended`. The player therefore ends the entry when `currentTime` comes within 0.25 seconds of that duration, or when `currentTime` stops advancing for 2.5 seconds while playing and within 2 seconds of it. Pausing, seeking or opening another source cancels the stall watch. Each source ends once, so a late `ended` event cannot advance the queue a second time. Repeat-one restarts the same recording, the queue end stops cleanly, and heard progress is the real `currentTime` near the end. A real network stall in the last 2 seconds can end an entry up to 2 seconds early.

During source recovery the replacement element has no metadata and reports an unknown duration. The player keeps the last known duration while the element has no metadata, so the seek bar does not shrink. A seek made in that window is stored as the pending position and applied on `loadedmetadata`; it is not written to an element that would discard it. Recovery resumes from the pending seek, else the element's position, else the last reported position, because a failed element can report zero.

Fourteen new cases in `playbackOverlongDuration.test.ts` and the existing playback suite pass in Docker. No WebKit browser check exists yet, because no fixture generator produces a VBR MP3 without a Xing header. Physical Safari acceptance remains open.

## Track changes with one audio element

The player now keeps one audio element for the whole session, including across natural track changes. Media Session metadata, playback state and position state are driven by the player state, not by the element, so they follow the swap: the title, artist and album change when the next entry is selected, the playback state is briefly `paused` between the ended track and the next `play` event, and the position restarts at 0 with the new duration. Tested with fake media only. Whether iOS or Android keep the lock-screen controls and background audio across the swap has not been verified on a physical phone. See [one persistent playing element](next-entry-preparation.md#one-persistent-playing-element).
