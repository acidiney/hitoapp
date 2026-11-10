# Audio conversion

The core can open an authorized local or server-managed file at a selected streaming quality. The HTTP routes and browser settings still serve the original representation. This capability is not deployed or qualified as a release feature yet.

## Quality policy

`PlaybackAccess::open_quality_audio` accepts a `QualityRequest` with one of these choices:

| Choice | Result when conversion is suitable |
| --- | --- |
| Original, the default | Original bytes and existing byte-range behavior |
| High | ADTS AAC with a target of 256 kbit/s |
| Data saver | ADTS AAC with a target of 128 kbit/s |

The bitrate is an encoder target. It is not a promise of an exact output byte count.

Known lossless audio is eligible for conversion. A known lossy codec is converted only when its audio-stream bitrate exceeds the target. A missing bitrate, unknown codec, or inconclusive profile preserves the original. The container bitrate is not used to guess the audio bitrate.

Remote provider streams remain original. This avoids starting a second unqualified network reader inside FFmpeg. Local originals and server-managed copies are opened through existing playback authority and private filesystem ownership. Conversion writes no persistent audio copy and changes no Download intent, library record, or original file.

## Process ownership

```mermaid
flowchart TD
    request[Authorized quality request] --> grant[Playback source grant]
    grant --> probe[Bounded profile worker]
    probe --> policy{Quality policy}
    policy -->|Preserve original| original[Existing authorized audio stream]
    policy -->|Convert| owner[Conversion owner]
    owner --> encoder[FFmpeg child]
    encoder --> channel[Bounded output channel]
    channel --> read[Grant checked before each delivery]
    revoked[Session or source revoked] --> owner
    closed[Consumer drops stream] --> owner
    owner --> join[Kill when needed and join child]
    join --> release[Release conversion capacity]
```

The owner watches source revocation and consumer closure even when the consumer does not request another chunk. It retains admission until its child has terminated. Reading the prefetched first chunk requires the same authority check as every later chunk. A failed encoder exit is an error, including when the process already emitted audio.

The probe has a ten-second deadline and a 16 KiB output bound. Conversion startup waits at most ten seconds for initial output. Later output reads have a fifteen-second bound, and normal EOF must be followed by successful child exit within two seconds. Output uses a channel of two 32 KiB chunks, providing backpressure rather than buffering the whole recording. The command requests one decoder thread and one encoder thread and retains the existing file/pipe protocol and input-format restrictions.

`CoreConfig::conversion_parallelism` defaults to two concurrent conversions and is clamped to one through eight. One account can hold at most three, subject to that host bound. These limits are independent of scan workers; setting scan parallelism does not alter playback admission. Scan and conversion processes can run at the same time, so their configured capacities add together.

Safe debug spans retain account and playback-grant correlation through asynchronous cleanup. Events report selected quality, outcome, bytes, and elapsed time without recording media names, paths, source URLs, or seek positions.

## Time offsets and delivery

A converted request can start at a time offset up to seven days. The resulting stream represents the remaining segment, not the whole recording. A HEAD request probes eligibility and describes conversion without starting an encoder.

The core may return `QualityAudio::Original` after an inconclusive profile or a converter startup failure. Its reason tells the caller why. An original result does not apply the conversion time offset. HTTP and browser integration must therefore agree on the effective representation before applying an offset. A converted media URL must fail rather than silently return original audio at zero. The browser must preserve the full recording duration, absolute progress, and paused or playing intent when it reopens at another offset.

## Evidence and remaining work

On 10 November 2026, Docker checks passed 51 public playback cases, including 19 conversion cases, one conversion-log privacy case, two inspection-command lifecycle regressions, and three admission-limit unit cases. New failing cases reproduced prefetched audio after logout, successful EOF after encoder failure, guessed conversion of an uncertain profile, and scan settings silently limiting conversion capacity. Those cases passed after the corrections. Scoped strict Clippy for core and the three affected test targets passed; workspace formatting is clean. Additional cases verify revocation during probing and child retirement without consumer polling.

Fixtures use temporary SQLite, deterministic owned audio, real FFmpeg/ffprobe and controlled failing tool wrappers. They cover both bitrate classes, time offsets, low-bitrate preservation, authentication, capacity, child cancellation, and safe logs. This evidence covers the core boundary in Linux Docker. It does not prove an authenticated HTTP workflow, real browser decoding/seeking, provider conversion, native quality settings, Windows/macOS conversion, packaging, live sources, or release performance. Those remain under the [release plan](release-plan.md).
