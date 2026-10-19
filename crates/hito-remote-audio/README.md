# Credential-free remote audio

This crate owns bounded HTTPS delivery of one caller-bound canonical PCM16 WAV. Provider bindings, current authority, metadata observations, upload acknowledgments, and unlock requests belong to its callers. Preserving a typed binding does not establish recording identity.

`MediaClient::admit` returns an opening capability with a private permit. The caller can acquire it before provider revalidation, then consume it with the private target, confirmed length, binding, and exact operator media origins. The returned body retains the permit until EOF, failure, or drop. Client owners share two media slots separately from provider API admission.

Media delivery rejects unapproved origins, nonpublic DNS answers, redirects, proxies, and invalid TLS or response framing. DNS work belongs to the current operation and retires on cancellation. The `network` module also supplies the same DNS and pinning policy used by provider control transports; those transports own credentials and control-body limits.

Opening validates the canonical WAV prefix and expected representation length. Pulls are bounded to 64 KiB with a 15-second idle deadline. Callers must consume through validated EOF; an HTTP relay must validate EOF before releasing its final declared bytes. No range, seek, conversion, persistent download, or provider retry is implemented here.

The binding and mapped-error type parameters preserve provider-specific public APIs. Shared DNS and body logs use static outcomes, counts, timings, and the current caller correlation span. They contain no target, binding, credentials, or response body.

Test-only TLS and DNS constructors require the `test-support` feature and owned loopback peers. They are not environment endpoint overrides.

The existing 22 RealDebrid adapter tests and six authenticated plugin-service tests pass after extraction, preserving their assertions, request counts, and private canaries. Strict all-target Clippy for this crate, the adapter, and the service passes with test support enabled; formatting also passes. An independent repeat and fresh source review pass on the same extraction. All 663 existing external lock package blocks remain byte-for-byte unchanged. AllDebrid media delivery is not yet implemented or qualified.
