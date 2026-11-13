# Credential-free remote audio

This crate owns bounded HTTPS delivery of caller-bound canonical PCM16 WAV or explicitly declared MPEG-1 Layer III audio. Provider bindings, current authority, metadata observations, upload acknowledgments, and unlock requests belong to its callers. Preserving a typed binding does not establish recording identity.

`MediaClient::admit` returns an opening capability with a private permit. The caller can acquire it before provider revalidation, then consume it with the private target, confirmed length, binding, and exact operator media origins. The returned body retains the permit until EOF, failure, or drop. Client owners share two media slots separately from provider API admission.

Media delivery rejects unapproved origins, nonpublic DNS answers, redirects, proxies, and invalid TLS or response framing. DNS work belongs to the current operation and retires on cancellation. The `network` module also supplies the same DNS and pinning policy used by provider control transports; those transports own credentials and control-body limits.

Opening checks the declared format against a bounded original prefix and the expected representation length. WAV retains its canonical 44-byte-header policy. MP3 requires two complete frames with matching sample rate and channel count; an optional ID3v2.3 or ID3v2.4 tag may contain at most 61,440 payload bytes within the 65,536-byte prefix budget. The descriptor does not infer MP3 duration or claim full-file validity. Upstream MIME and filename extensions do not establish the format.

Pulls are bounded to 64 KiB with a 15-second idle deadline. Callers must consume through validated EOF; an HTTP relay must validate EOF before releasing its final declared bytes. No range, seek, conversion, persistent download, or provider retry is implemented here.

The binding and mapped-error type parameters preserve provider-specific public APIs. Shared DNS and body logs use static outcomes, counts, timings, and the current caller correlation span. They contain no target, binding, credentials, or response body.

Test-only TLS and DNS constructors require the `test-support` feature and owned loopback peers. They are not environment endpoint overrides.

Real-Debrid can select MP3 explicitly; AllDebrid, Premiumize and TorBox still select WAV. All six authenticated MP3 HTTP cases pass with exact original bytes and native decoding through EOF. Compatibility fixtures cover 48 kHz at 128 kbit/s with tagless stereo, ID3v2.3 stereo, ID3v2.4 stereo with/without a footer, and ID3v2.4 mono. Scoped test Clippy passes. The preceding product slice also passed fifty named client/executable regressions, WAV boundaries, strict checks, three source reviews and an independent MP3 repeat. Dependencies are unchanged. VBR, other rates/bitrates and complete application playback still need release qualification. See [provider behavior and limits](../../docs/guide/plugins.md).
