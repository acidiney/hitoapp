# Native audio engine

`hito-audio` decodes an owned input with Symphonia and, with its `device` feature, sends PCM through CPAL. The [native playback owner](native-playback.md) now supervises authorized local/provider input and session revocation around this engine. The Tauri shared UI uses authenticated native commands and observations. Rust owns queues, source recovery, output, and listening progress; ordinary web sessions retain browser playback.

## Ownership

The engine accepts an owned `Input`, including `std::fs::File`, with read/seek and length capabilities. It accepts no path, URL, account string, or renderer-supplied authority. A trusted input can perform I/O; `hito-playback::Source` obtains it through core's `PlaybackAccess` and retains its [playback access grant](playback-access.md). An opened file alone does not provide revocation.

```mermaid
flowchart LR
  Access[Core PlaybackAccess / authorized grant] --> Session[hito-playback session and owned input]
  Session --> Player[hito-audio Player]
  Player --> Decoder[Owned decoder worker / Symphonia]
  Decoder --> Convert[Converter / downmix to stereo, decimate above 192 kHz]
  Convert --> Tempo[Worker-owned SoundTouch / bounded tempo segments]
  Tempo --> PCM[Bounded 100 ms PCM ring / generation, source time and rate tags]
  PCM --> Render[CPAL callback / fixed presentation queue]
  Render --> Device[Native output device]
  Render --> State[Atomic position, completion and failure state]
  State --> Player
  Player --> Join[Close or Drop / stop and join worker, release output]
```

`Decoder` exposes the actual decoded sample rate and channel count, interleaved float samples, seeking, and interruptible packet scanning. It preserves high-resolution samples. It rejects nonfinite PCM and changes in rate or channel count during a stream. Public errors contain static categories.

`Player` opens one input, starts paused, and owns resume, pause, seek, volume, speed, snapshots, and cleanup. Its worker owns both the decoder and output stream. Construction and control methods block. The native session owner calls them outside Tokio's asynchronous worker threads. Seeking waits for the decoder to apply the target, preserves paused state, and changes the PCM generation. The callback discards older generations and preserves frames newer than its current observation.

The output starts with silence and waits for two callbacks before returning readiness. This primes the device before application audio. Device selection requests a 20 ms period within its advertised bounds; the negotiated callback size may differ. Output requires the negotiated representation's exact rate and one or two channels, with float32, signed 16-bit, or unsigned 16-bit samples. The [output negotiation](#output-negotiation) chooses that representation from what the device offers and the [conversion stage](#conversion-and-speed-capability) produces it.

The PCM ring holds 100 ms of frames. The callback performs no allocation, locking, decoding, filesystem access, networking, or logging. Its fixed presentation queue holds at most 1,024 points. Overflow becomes a typed output failure. Stale-frame work is bounded by ring capacity for each callback.

## Output negotiation

Before the converter is built, the worker asks the default output device which float32, signed 16-bit or unsigned 16-bit configurations it offers (a channel count with a sample-rate range) and which rate it prefers. `negotiate::choose` is a pure function over that list:

1. A source rate (after the reduction to 192 kHz described below) and channel count that the device offers are kept unchanged. The ordinary case is untouched: nothing is modified at 1.0.
2. Otherwise the target stays in the source's rate family (44.1 kHz: 11.025, 22.05, 44.1, 88.2, 176.4; 48 kHz: 8, 12, 16, 24, 32, 48, 96, 192). The device default is used if it is in the family, else the highest offered family rate not above the source (the lowest above it when nothing lower is offered).
3. A source outside both families, or a family the device does not offer, uses the device default rate, then the offered rate nearest to the source.
4. A mono source on a device that offers only stereo is duplicated to both channels. Sources with more than two channels are mixed to stereo first.

If the device offers no usable configuration for the source's channel count the open fails with the typed `OutputFormat` error ("This audio device can't play this file's format"). A missing device or an unreadable configuration list stays `OutputUnavailable`. The outcome (source rate and channels, number of offered configurations, device default rate, chosen rate and channels) is logged at debug level without paths or titles. `Player::output_format` reports the negotiated representation. `Player::snapshot().format` and the speed capability describe the source; the capability stays 0.5 to 3.0 because the converter produces the negotiated representation before time-stretching.

An open failure reaches the playback owner as a typed session error: the session fails, the source is marked failed, and the queue follows the existing exhausted-source handling. When the cause is `OutputFormat`, the owner keeps it through the source cycle, so a later lookup or refresh failure (`not_found`, `unavailable`) does not replace it; only a withdrawn authorization does. The wire error is `output_format`, reported in `last_failure.error` for the skipped track (the player `error` field is cleared when the queue moves on, as for every other failure). Every other native failure stays `playback`.

Two players show two strings. The native player, whose output belongs to the host, shows "The audio output can't play this format." (for a skipped queued track: "The audio output can't play a queued track's format."). The browser player shows "This device can't play this audio format." only after the media element reported `MEDIA_ERR_SRC_NOT_SUPPORTED` (or `play()` rejected with `NotSupportedError`) for the current source and a ranged `GET` of that source (`bytes=0-0`, four second limit) came back 2xx with an `audio/*` or `application/octet-stream` content type. A missing, expired, blocked or slow source keeps "Audio is unavailable for …". Neither message carries the sample rate or channel count; those stay in the debug log. Tests restrict the device with `hito_audio::test_support::restrict_output_rates`, which does not exercise CoreAudio.

## Conversion and speed capability

Each decoded source has one capability, derived from its actual decoded sample rate and channel count before the output opens. `AudioFormat::speed_capability` reports either a range (0.5 to 3.0) or unit-only speed, and `Player::speed_capability` exposes it for the opened source. Every representation the decoder accepts has the full range, because the engine converts it first. The accepted representations are 1 to 32 channels at 8 to 192 kHz, or at one of the four standard high rates 352.8, 384, 705.6 and 768 kHz. `capability::supported` holds these bounds and both the decoder and the capability use it, so they cannot disagree. A rate above 192 kHz outside those four (for example 200, 220.5 or 250 kHz) would decimate onto a rate that output devices do not open, so the decoder rejects it with the typed `Unsupported` error instead of failing later at the device, and `AudioFormat::speed_capability` reports unit-only for it. No decoder-accepted source reports unit-only, so the unit-only path is exercised by owner unit tests and the browser path rather than by a native audio fixture.

`Converter` sits between the decoder and the time-stretcher and is the only place sample rate or channel count changes:

- A source at or below 192 kHz with one or two channels is forwarded unchanged: no sample is modified, so 1.0 playback of those sources is bit-exact.
- A source above 192 kHz is decimated by an integer factor (two for 352.8 and 384 kHz, four for 705.6 and 768 kHz, so the results are 176.4 kHz or 192 kHz, always in the 44.1 or 48 kHz family) with a 129-tap linear-phase Kaiser-windowed low-pass filter. Tests measure 1 kHz and 10 kHz tones at 384 kHz, a 3 kHz tone at 705.6 and 768 kHz kept within 2 percent of their amplitude and a 150 kHz tone, which would fold to 42 kHz, rejected below 0.2 percent of full scale (about 54 dB), so out-of-band content cannot fold into the audible band. They do not characterise the full passband ripple or compare against a reference resampler. The filter is centred on each output frame, so media time needs no latency offset, and trailing silence flushes the final frames at the end of the stream. This conversion applies at every speed, including 1.0: a stream cannot change its output rate while playing, and most output devices do not accept rates above 192 kHz.
- When the device does not offer the reduced rate, the converter goes straight from the source rate to the negotiated rate in one stage, except for 705.6 and 768 kHz sources going to a rate below 176.4 or 192 kHz. Those use two stages: a short integer decimator by four (65 taps, Kaiser beta 9, cut-off at 45 percent of the 176.4 or 192 kHz rate) and then the polyphase stage described below from 176.4 or 192 kHz to the device rate. The short first filter is enough because the second stage removes everything near the device Nyquist rate, and the first only has to reject content that would fold into the audible band. It lowers the filter length per output frame from about 940 to 1,025 taps to about 475 to 516 (705.6 to 48 kHz: 942 to 475, 768 to 48 kHz: 1,025 to 516, 705.6 to 44.1 kHz: 1,025 to 516; counted by a unit test, not timed). Every other rate pair keeps its single stage. For the single-stage cases, an integer ratio (384 kHz to 48 kHz is a factor of 8, 192 kHz to 48 kHz a factor of 4) uses the same decimator with a filter that scales with the factor. Any other ratio (192 kHz to 44.1 kHz, 176.4 kHz to 48 kHz, 44.1 kHz to 48 kHz) uses a polyphase bank of Kaiser-windowed sinc filters (beta 9, 32 zero crossings of the lower rate on each side, one filter per fractional position, normalised to unity gain at DC). The low-pass sits at 90 percent of the lower Nyquist rate. The number of filters is the reduced output/input ratio denominator, rounded to the nearest of 4,096 positions when it would be larger. Both stages are centred on each output frame, hold only their own history plus one packet, run on the decoding worker rather than the output callback, and restart on seek. Tests cover 176.4 to 48, 192 to 44.1, 96 to 48, 44.1 to 48, 384 to 48, 352.8 to 44.1 and 48 to 44.1 kHz: exact frame counts, a 1 kHz tone within 1 percent of its level, a tone above the target Nyquist rejected below 0.05 percent of full scale (about 60 dB), seek and end-of-stream behaviour, and bit-exact forwarding when no conversion is requested. The two-stage pairs (705.6 to 48 and 44.1 kHz, 768 to 48 and 96 kHz) have their own conversion tests: exact frame counts, 1, 10 and 18 kHz tones within 1 percent of their level and 30, 60, 100, 150, 200 and 300 kHz tones, which would fold into the output band, rejected below 0.05 percent of full scale.
- A source with more than two channels is mixed to stereo using its speaker positions. Front channels keep their side, centre channels feed both sides at -3 dB, surround and rear channels feed their side at -3 dB, and low-frequency channels are dropped. These ITU-R BS.775 style coefficients are not normalised, so a front-only signal keeps its stereo level (a front-only 5.1 tone measures the same as the same tone in stereo). A mix with content in many channels can sum above full scale, so a peak limiter follows the downmix: its gain is exactly 1 until a frame would pass full scale, drops at once to hold that frame at full scale, and returns to 1 over about 150 ms. Output samples of a downmix therefore never exceed full scale, and quiet or front-only material is not altered. The limiter is not applied to sources that are not downmixed. A layout without speaker positions (for example AIFF with more than three channels) uses the default speaker order for its channel count: 3 front left, front right, centre; 4 front pair and rear pair; 5 front pair, centre and rear pair; 6 front pair, centre, low-frequency and rear pair; 7 front pair, centre, low-frequency, rear centre and side pair; 8 front pair, centre, low-frequency, rear pair and side pair. Counts above eight alternate channels between left and right. Surround content therefore plays at every speed instead of failing at the output.

The processing is bounded: the filter keeps only its own history plus one decoded packet, output blocks are one packet, and cancellation reaches the decoder between packets exactly as before. Converted media time follows the decoder's packet times: each segment starts at a packet time, and a packet that does not continue the previous one by more than one converted frame starts a new segment, so a timeline gap is not smeared into drift. Positions never exceed the source duration. Seeking resets the filter and the limiter and restarts media time at the decoder's reported position; the first few output frames after a seek carry a filter transient shorter than a millisecond.

```mermaid
flowchart LR
  Open[Decoder opens / actual rate and channels] --> Capability[Speed capability / range or unit only]
  Capability --> Owner[Native control module / negotiate chosen speed]
  Owner -->|supported| Chosen[Apply chosen speed]
  Owner -->|unit only| Unit[Present at 1.0, keep chosen speed]
  Chosen --> Snapshot[Player snapshot: speed, effective_speed, speed_support]
  Unit --> Snapshot
  Snapshot --> UI[Shared UI / disabled options and a non-blocking notice]
  Browser[Browser media element / verify accepted playbackRate] --> Snapshot
```

The native control module in `hito-playback` negotiates for each opened session. It applies the chosen speed when the source's capability supports it and otherwise leaves the session at unit speed without failing it. The chosen speed stays in the owner, so a later capable source applies it again. The owner snapshot reports `speed` (the chosen speed), `effective_speed` (derived by the owner from the chosen speed and the source's reported support, so it changes in the same snapshot as a speed command and engine updates cannot revert it) and `speed_support` (the range in thousandths, `1000`-`1000` for unit-only, absent while unknown). The browser player verifies after each source loads that the media element accepted the rate; a refused or throwing rate returns the element and its default rate to 1.0 and reports unit-only for that source, and the next source retries the chosen speed. Both players expose `speedRange` and `effectiveSpeed` through the shared player interface. The speed menu disables choices outside the range, and when the effective speed differs from the chosen one a non-blocking status line says that the source plays at 1× and that the chosen speed returns with a source that supports it.

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

The bundled processor rejects sample rates above 192 kHz, so the [conversion stage](#conversion-and-speed-capability) hands it at most 192 kHz and at most two channels. A request outside the source's capability is rejected before changing its rate or playback state. Monitor fixtures cover tempo at 8, 16, 44.1, 48, 96, and 192 kHz, and converted 384 kHz, 352.8 kHz, 705.6 kHz, 768 kHz and 5.1 sources at unit speed, 2.0 and 0.5 (the 705.6 and 768 kHz fixtures are in `tests/tempo_high_rate.rs`, run as their own monitor process by `scripts/check-audio.sh`; they are generated at test time as short stereo or mono tones, also on a device restricted to 48 kHz, and check completion position, played-frame count and pitch), including mid-playback speed changes, seeking and cleanup.

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

The Linux device check uses an unprivileged process, private temporary PulseAudio socket, clocked null sink, and its monitor. It measures real CPAL output for tone identity, pause silence, backward/zero seeking, volume, short-clip completion, restart after end, explicit close/Drop, underrun recovery at 2.0, and exact sample continuity across prepared inputs (see [gapless transitions](#gapless-transitions)). Tempo checks measure preserved pitch and output duration (including converted high-rate and surround sources), source-time progression, pause/seek behavior, forward-only input, rapid rate reversal, and final observed rate. It never mounts the normal server volume or personal music. Renderer and end-marker races also have deterministic callback-boundary tests.

Vendor diagnostics are suppressed only around Symphonia calls because malformed media can enter its log messages. Hito retains safe decoder readiness, seek, speed requests, tempo transitions, output, recovery, failure, and cleanup events. Player spans use a process-local audio ID. No media bytes or private paths enter those events.

## Gapless transitions

`Player::prepare_next(input)` opens a second decoder and hands it to the running worker. When the current input reaches its end, the worker continues into the prepared input in the same output stream, the same ring and the same callback. No output stream is closed or reopened, no priming silence is inserted, and the next input's first sample follows the current input's last sample. `Player::cancel_next()` withdraws a prepared input that has not started. A second `prepare_next` replaces the first.

```mermaid
flowchart LR
  A[Current decoder] --> Conv[Converter to the device rate and channels]
  B[Prepared decoder] --> Conv2[Converter to the same rate and channels]
  Conv --> Tempo[One tempo stage]
  Conv2 -->|joined at end of input| Tempo
  Tempo --> Ring[Frames tagged with track number]
  Ring --> Callback[Callback publishes position and track together]
```

Rules.

- A prepared input must convert to the open stream's sample rate and channel count. The stream keeps the representation negotiated for its first input, so a different source rate is resampled and a mono or surround source is mixed. An input that cannot match the channel count returns `FormatChanged`. An input offered after the stream ended returns `Interrupted`, and the caller falls back to opening a new player.
- The tempo stage is not flushed at the join. The next input enters the same processor, so a rate other than 1.0 keeps its pipeline across the boundary. Output frames are tagged with a track number counted on the output side of that processor. The number changes after exactly the frames that belong to the first input, so the boundary is not early by the processor's latency.
- Frames carry the track number to the callback. The callback publishes track, position, rate and completion as one observation, and it schedules the old track's last point before the new track's first point even inside one callback buffer. `Snapshot::track` counts presented transitions and `Snapshot::format` describes the presented track. Position restarts at the new input's own time. Completion is published once, for the last input.
- Offering or withdrawing an input never discards decoded audio. Controls that change the stream (seek, stop) still discard queued blocks, but `prepare_next` and `cancel_next` are applied between blocks.
- A seek aimed at the old track after the worker moved on, or while the join is still draining through the processor, is accepted and ignored. The queue owner has already seen the natural advance. Output recovery after an underrun in that short window cannot re-seek the old input and ends the player with `Output`; the native owner then recovers through its existing source attempts.
- Once the worker joins the prepared input it can no longer be withdrawn. That point is up to the ring depth (100 ms) plus the tempo latency before the old track is audible to its end.

### Measured gaps

Silence is the longest run of exactly zero samples between the first and last captured sample of a fixture whose samples are a counter that never equals zero. The clocked PulseAudio monitor captures the left channel at 48 kHz. Before is the previous behavior: close the player at the end of the first input and open another.

| Case | Before | After |
| --- | --- | --- |
| Engine, WAV to WAV, reopening the player | 4,089 to 4,930 frames (85 to 103 ms, varies between runs) | 0 frames, every sample equal |
| Engine, FLAC to FLAC | not measured separately | 0 frames, every sample equal |
| Engine, 440 Hz sine split across two files | not measured separately | 0 frames, no phase error above one 16-bit step |
| Engine, 44.1 kHz second input into a 48 kHz stream | not measured separately | 0 frames, 48,000 frames for 24,000 plus 22,050 source frames |
| Engine, speed 0.5 and 2.0 | not measured separately | 1 frame (a counter wrap that the processor smoothed to zero), no extra padding |
| Queue, two recordings, native player | 5,431 to 7,045 frames (113 to 147 ms, varies between runs) | 0 frames, every sample equal |
| Queue, a recording followed by itself | 5,195 frames (108 ms, one run) | 0 frames, every sample equal |
| Queue, speed 2.0 | 2,400 frames or more | 1 frame |

Frame counts depend on machine load before the change because the gap was the time to resolve, open and prime a new stream. After the change the count is the same on every run.

### Codec matrix

Gapless depends on what each demuxer and decoder removes. FFmpeg 5.1 generated a 24,000-frame sine for each row. Rows marked exact decode to exactly 24,000 frames, so a join adds nothing. The gap column is the longest run of near-silent samples (below 0.0005) within 4,800 frames of the join on the clocked monitor.

| Input | Decoded length before | Decoded length now | Join gap now | Reason |
| --- | --- | --- | --- | --- |
| WAV PCM | exact | exact | 0 frames | no encoder delay |
| FLAC | exact | exact | 0 frames | no encoder delay |
| ALAC in M4A | exact | exact | 0 frames | no encoder delay |
| MP3 with a Xing or LAME header | exact | exact | 1 frame | Symphonia reads the header and its decoder trims |
| Ogg Vorbis | exact | exact | 1 frame | the Ogg demuxer computes both from granule positions |
| AAC in M4A with an edit list (FFmpeg default) | 25,600 (1,600 too long) | exact | 1 frame (was 183) | Hito reads the `elst` atom and trims the decoded output |
| AAC in M4A with an iTunSMPB atom and no edit list | 25,600 (1,600 too long) | exact | 1 frame (was 183) | Hito reads priming and length from `iTunSMPB` and trims |
| AAC in M4A with neither | 1,600 too long | 1,600 too long | 183 frames | nothing declares the delay |
| AAC in ADTS | 1,600 too long | 1,600 too long | 183 frames | ADTS has no container-level delay field |
| MP3 without a Xing or LAME header | 1,344 too long | 1,344 too long | about 1,300 frames | no delay information; no reliable signal to apply a standard delay |
| Opus | unsupported | unsupported | not applicable | Symphonia 0.6.1 has no Opus decoder; the FFmpeg fallback does not exist yet |

One frame is a sample at a sine zero crossing, the same value the exact codecs show.

### M4A trimming

`Decoder::open` reads the MP4 structure of a seekable input before probing, then rewinds. The reader in `mp4_gapless.rs` takes the first audio track's `mdhd` timescale and `elst` entries, the `mvhd` timescale, and the `iTunSMPB` freeform atom in `moov/udta/meta/ilst`. Symphonia still demuxes and decodes. The decoder drops the declared priming frames, stops after the declared length, reports the trimmed length as the duration, and counts positions and seeks on the trimmed timeline. A seek starts decoding about 2,048 source frames early so the first returned frame has its overlap context.

Rules.

- One non-empty edit entry with a positive duration gives the start (`media_time`) and the length (`duration` in movie timescale, converted to frames). Empty edits, several entries, zero timescales and more than 64 entries are not used.
- `iTunSMPB` gives priming and the original length in exact frames. When both exist and agree within one movie-timescale tick, `iTunSMPB` supplies the length, because an edit list stored in a coarse movie timescale can be off by a few frames. When they disagree, the edit list wins.
- A declaration whose start plus length exceeds the stream is ignored, and the file plays untrimmed. A shorter declaration is honored.
- Parsing is bounded: at most 4,096 atoms, leaf atoms read at most 4,096 bytes, 64 edit entries, and only the fixed nesting `moov/trak/(edts|mdia)`, `moov/udta/meta/ilst/----`. Atom sizes must fit in the parent, 64-bit sizes are checked for overflow, and any structural fault gives no trim instead of an error. Inputs that are not seekable are not parsed.
- Seeking after the stream was read to its end fails in Symphonia's MP4 demuxer for any M4A, trimmed or not. This work does not change that.

The fixtures come from FFmpeg 5.1 (`-use_editlist 0` removes the edit list) and a test helper that appends an `iTunSMPB` atom to `moov`. The device test records the join gap for every row above.

## Remaining integration

The [application coordinator](native-coordinator.md) supplies authorized recording/source IDs, stable queue entries, bounded source attempts, progress persistence, and account/session command fencing. The desktop retains native cleanup through shutdown. Its HTTP adapter reuses server authentication and local process protection. React sends commands and observes snapshots through the shared player interface; web sessions keep the browser implementation. FFmpeg fallback, resampling to arbitrary device-supported rates, and platform/hardware acceptance remain unfinished.

FFmpeg fallback, delay trimming for ADTS, untagged M4A and untagged MP3, device selection/media controls, platform verification, and release latency targets remain unfinished. Linux monitor evidence does not establish speed quality across all codecs, physical devices, or platforms. The [release plan](release-plan.md) and [TODO](../TODO.md) retain those requirements.

The pinned [Symphonia documentation](https://docs.rs/symphonia/0.6.1/symphonia/) lists codec and gapless limitations. [CPAL](https://docs.rs/cpal/0.18.2/cpal/) defines callback/device behavior, and [rtrb](https://docs.rs/rtrb/0.4.0/rtrb/) supplies the fixed-capacity SPSC ring.

## High-rate monitor evidence

The 705.6 kHz and 768 kHz sources are accepted, decimated and played on the Linux clocked monitor at 1.0, 0.5 and 2.0 on the default device and on a device that offers only 48 kHz, with the reported position matching the source duration to 1 ms. On the 48 kHz-only device the converter used to resample straight from the source rate (about 940 to 1,020 filter taps per output frame), and at 2.0 an unoptimised test build could not sustain that in real time: the stream underran and its tone measured 452 Hz instead of 440 Hz. The two-stage conversion halves that filter length, and the 2.0 case now passes in the unoptimised test profile (it failed before the change). On the default device the 2.0 case uses a mono source: a 768 kHz stereo source at 2.0 (plain four-times decimation to 192 kHz) is CPU-bound in the unoptimised test build and intermittently underran with and without the two-stage change, so stereo is covered at 1.0 and the unit-speed test, and 2.0 with mono (10 of 10 isolated runs passed). This is monitor evidence only. Real hardware at these rates, browser playback and CPU measurements on the MacBook Pro are still open in the [TODO](../TODO.md).
