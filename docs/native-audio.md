# Native audio engine

`hito-audio` decodes an owned input with Symphonia and, with its `device` feature, sends PCM through CPAL. The [native playback owner](native-playback.md) now supervises authorized local/provider input and session revocation around this engine. The Tauri shared UI uses authenticated native commands and observations. Rust owns queues, source recovery, output, and listening progress; ordinary web sessions retain browser playback.

## Ownership

The engine accepts an owned `Input`, including `std::fs::File`, with read/seek and length capabilities. It accepts no path, URL, account string, or renderer-supplied authority. A trusted input can perform I/O; `hito-playback::Source` obtains it through core's `PlaybackAccess` and retains its [playback access grant](playback-access.md). An opened file alone does not provide revocation.

```mermaid
flowchart LR
  Access[Core PlaybackAccess / authorized grant] --> Session[hito-playback session and owned input]
  Session --> Player[hito-audio Player]
  Player --> Decoder[Owned decoder worker / Symphonia]
  Decoder --> Tempo[Worker-owned SoundTouch / bounded tempo segments]
  Tempo --> PCM[Bounded 100 ms PCM ring / generation, source time and rate tags]
  PCM --> Render[CPAL callback / fixed presentation queue]
  Render --> Device[Native output device]
  Render --> State[Atomic position, completion and failure state]
  State --> Player
  Player --> Join[Close or Drop / stop and join worker, release output]
```

`Decoder` exposes the actual decoded sample rate and channel count, interleaved float samples, seeking, and interruptible packet scanning. It preserves high-resolution samples. It rejects nonfinite PCM and changes in rate or channel count during a stream. Public errors contain static categories.

`Player` opens one input, starts paused, and owns resume, pause, seek, volume, speed, snapshots, and cleanup. Its worker owns both the decoder and output stream. Construction and control methods block. The native session owner calls them outside Tokio's asynchronous worker threads. Seeking waits for the decoder to apply the target, preserves paused state, and changes the PCM generation. The callback discards older generations and preserves frames newer than its current observation.

The output starts with silence and waits for two callbacks before returning readiness. This primes the device before application audio. Device selection requests a 20 ms period within its advertised bounds; the negotiated callback size may differ. Output currently requires the source's exact rate and one or two channels, with float32, signed 16-bit, or unsigned 16-bit samples. No resampling or channel conversion is implemented yet.

The PCM ring holds 100 ms of frames. The callback performs no allocation, locking, decoding, filesystem access, networking, or logging. Its fixed presentation queue holds at most 1,024 points. Overflow becomes a typed output failure. Stale-frame work is bounded by ring capacity for each callback.

## Tempo and media time

Native speed accepts finite rates from 0.5 to 3.0. The decoder worker uses bundled SoundTouch 2.3.2 through a private C ABI adapter and the pinned `soundtouch-ffi` library to change tempo while preserving pitch. A 1.0 rate bypasses processing. C++ constructs and deletes each processor; Rust owns its opaque handle through unconditional Drop. The C ABI contains native exceptions. SoundTouch owns its processing buffers on the worker; the callback only reads prepared frames. The dependency and license are recorded in [third-party notices](third-party-notices.md).

Each transition drains the current processor before starting the latest requested rate. The adapter calculates the expected segment output from genuine input frames and tempo, supplies bounded silence using the configured initial latency and input sequence size, verifies that the expected output exists, and trims padding before delivery. Failure to drain is an output error, never a successful empty ending. Padding does not advance media time. Already prepared audio keeps its original rate tag. The worker retains any unread raw samples, including when returning to 1.0 or reversing a pending transition. Changing speed neither seeks the input nor changes play/pause, so forward-only streams remain usable. Processing consumes chunks of at most 10 ms and returns at most 4,096 frames at a time. Input reads and output buffering can delay the rate becoming audible; a successful control reply acknowledges the request, not its presentation.

```mermaid
flowchart LR
  Source[Decoded source frames / media time] --> Segment[Tempo segment / captured rate]
  Segment --> Output[Processed output frames / device time]
  Output --> Tags[PCM with source position and actual rate]
  Tags --> Callback[Callback presentation timeline]
  Callback --> Observed[Observed source position and rate]
```

Nonunit tempo is unsupported above 192 kHz because the bundled processor rejects those sample rates. The engine rejects such a speed request before changing its rate or playback state, preserving unit-speed playback. The coordinator currently reports a visible playback failure after finite source attempts; choosing 1.0 and Resume can retry the same paused occurrence. Source-specific speed negotiation and seamless rejection while preserving active playback remain unfinished. Monitor fixtures cover tempo at 8, 16, 44.1, 48, 96, and 192 kHz, with 384 kHz rejection followed by actual unit-speed output.

Positions, seeks, and duration use source media time. At 2.0, four seconds of source audio produce about two seconds of device audio. Each frame carries its source cursor and segment rate. Snapshots publish that rate when the corresponding audio is estimated to have played. A completion marker only advances position/completion and preserves the last presented rate, including a short clip ending within one callback or a seek directly to the end. Before any audio is presented, the observed rate starts at 1.0. Seeking resets the processor and generation while retaining the latest requested rate; output recovery uses the same reset at the last presented cursor.

## Position and completion

Decoder read-ahead does not advance playback position. The callback schedules source positions and the ordered end marker against CPAL's playback timestamps. Consecutive buffers cannot overlap, including the initial silent buffers. Later callbacks publish points after their estimated presentation time. One callback-owned atomic sequence publishes position, rate, and completion together. Worker seeks/recovery use a separate sequence for the base cursor and generation. Snapshot readers retry if either publication is incomplete or changed; the callback takes no lock. Completion therefore follows the final marker rather than an empty producer observation.

This is an estimate from the selected backend's clock, not proof of physical speaker timing. The Linux monitor test verifies the emitted signal. Other drivers and platforms need their own acceptance checks.

The Loom model uses the actual `State::present`, `reset`, and single observation-read attempt with modeled atomics. It checks presentation, resetting an ended observation, and a retiring callback concurrent with reset. Each writer performs one operation and the reader makes one attempt, with at most two thread preemptions and a [1,000-thread-switch execution limit](https://docs.rs/loom/0.7.2/loom/model/struct.Builder.html#structfield.max_branches). It checks observation consistency rather than progress of the retry loop. [Loom cannot model every relaxed-memory execution](https://docs.rs/loom/0.7.2/loom/#relaxed-memory-ordering), so this is bounded regression evidence, not proof for every platform. Run it separately with `sh scripts/check-audio-model.sh`; it uses `/build/loom-target` and does not compile CPAL, SoundTouch, or Tauri.

```mermaid
sequenceDiagram
  participant Owner as Player owner
  participant Decode as Decoder worker
  participant Ring as PCM ring
  participant Device as CPAL callback and device
  Owner->>Decode: Seek to requested position
  Decode->>Decode: Reset decoder and increment generation
  Decode->>Ring: New generation frames
  Decode-->>Owner: Seek applied
  Device->>Ring: Discard older frames, retain newer frames
  Decode->>Ring: Ordered end marker after final sample
  Device->>Device: Submit audio and schedule presentation
  Device->>Owner: Publish position and completion after presentation time
  Owner->>Decode: Stop
  Decode->>Device: Release output stream
  Decode->>Decode: Release decoder and input
  Decode-->>Owner: Worker joined after cleanup
```

An ALSA underrun is recoverable. Its callback records a counter. The worker releases the broken stream, seeks to the last presented position, opens a fresh output stream and PCM ring, and acknowledges recovery. It allows at most three consecutive resets without 200 ms of playback progress. The output suppresses old queued audio until acknowledgment and resets its presentation timeline. Error handling is scoped to each stream so a fatal replacement error cannot disappear during recovery. Other device failures stop output and remain visible in snapshots. Full source failover belongs to the planned coordinator.

`stop_handle()` returns a cloneable terminal stop request independent of the player lock. It silences callbacks even while the worker is blocked, and prevents later resume or seek. Close and Drop request stop and join the worker. It releases the decoder, input, and output stream before the join returns. Packet scanning checks interruption between demuxed packets, including packets for other tracks. A blocked operating-system read, internal container seek, or driver call cannot be forcibly interrupted by this implementation. Real disconnected-drive behavior remains a release gate.

## Verification

After building the Docker tools images, run:

```sh
docker compose --profile tools build rust
docker compose --profile desktop build desktop
sh scripts/check-audio.sh
```

The desktop verification script also runs this check. Ordinary workspace builds keep CPAL disabled and need no ALSA development package.

The hardware-independent tests cover exact WAV samples, backward and zero seeks, nonfinite/truncated input, 24-bit precision, cancellation during a video-only tail, and trace-level log privacy. FFmpeg generates deterministic PCM16/PCM24/float WAV, FLAC, MP3, Vorbis, AAC-LC, ALAC, AIFF, and CAF fixtures. These checks establish decoding for those profiles, not every profile of each codec. Opus explicitly returns unsupported until fallback exists.

The Linux device check uses an unprivileged process, private temporary PulseAudio socket, clocked null sink, and its monitor. It measures real CPAL output for tone identity, pause silence, backward/zero seeking, volume, short-clip completion, restart after end, explicit close/Drop, and underrun recovery at 2.0. Tempo checks measure preserved pitch and output duration, source-time progression, pause/seek behavior, forward-only input, rapid rate reversal, and final observed rate. It never mounts the normal server volume or personal music. Renderer and end-marker races also have deterministic callback-boundary tests.

Vendor diagnostics are suppressed only around Symphonia calls because malformed media can enter its log messages. Hito retains safe decoder readiness, seek, speed requests, tempo transitions, output, recovery, failure, and cleanup events. Player spans use a process-local audio ID. No media bytes or private paths enter those events.

## Remaining integration

The [application coordinator](native-coordinator.md) supplies authorized recording/source IDs, stable queue entries, bounded source attempts, progress persistence, and account/session command fencing. The desktop retains native cleanup through shutdown. Its HTTP adapter reuses server authentication and local process protection. React sends commands and observes snapshots through the shared player interface; web sessions keep the browser implementation. Saved-position loading, resampling, FFmpeg fallback, gapless, and platform/hardware acceptance remain unfinished.

FFmpeg fallback, resampling, channel conversion, gapless preparation, device selection/media controls, platform verification, and release latency targets remain unfinished. Linux monitor evidence does not establish speed quality across all codecs, physical devices, or platforms. The [release plan](release-plan.md) and [TODO](../TODO.md) retain those requirements.

The pinned [Symphonia documentation](https://docs.rs/symphonia/0.6.1/symphonia/) lists codec and gapless limitations. [CPAL](https://docs.rs/cpal/0.18.2/cpal/) defines callback/device behavior, and [rtrb](https://docs.rs/rtrb/0.4.0/rtrb/) supplies the fixed-capacity SPSC ring.
