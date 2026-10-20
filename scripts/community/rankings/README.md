# Packaged community ranking gate

Run `run-packaged.sh chromium` or `run-packaged.sh webkit` with exact app and
community image IDs, source commits, and input fingerprints in the six required
`HITO_PACKAGED_*` and `HITO_RANKING_SERVICE_*` environment variables. The runner
checks those pins, creates a fresh Compose project with shared loopback and no
external network or persistent volumes, and keeps its evidence after cleanup.
The app image must match the current product source; an older app image cannot
qualify new UI or runtime behavior.

The journey uses real provider adoption, a real community service, and owned WAV
bytes. It checks default and unsaved no-outbound behavior, a separately saved
ranking preference, ordinary Play selection, PCM decoding, heard progress
receipts, unchanged reporting flags, private state and originals, logout, and
bounded canonical community requests. Passing this gate does not qualify native
ranking consumption, refresh/resolve ranking, public deployment, or the whole
release.

The application uses the logout session check's `/api/v1/me` 401 status without
reading its body. Only the exact GET begun after logout is eligible, at most
once, with mandatory JSON, no-store, UUID correlation, and declared length of 31
bytes. Its native console diagnostic must match that same response exactly.
The gate attempts Playwright's original body read for five seconds. A completed
body must have the exact length and canonical error. If both request and response
are still pending, with no failure, abort, completion event, or response-end
observation, the evidence explicitly records `bodyObservation: unobserved` and
`bodyReason: consumption_sensitive`. It does not claim the body was checked or
infer a server defect. Any reader error or other completion state fails.
No fetch wrapper, clone consumer, or refetch is installed by the acceptance gate.

Only the exact owned recording's same-origin GET artwork 404, without query or
fragment, is eligible. Every accepted response must have a canonical 30-byte
body, JSON/no-store headers, UUID correlation, and one matching native console
message. The finite ceiling is 14: three full Playlist page loads with four known
recording image slots each, plus two Settings page loads with one dock slot each.
The Playlist slots are its hero, single track row, current ListeningPanel image,
and current dock image. The one-entry queue has no upcoming image, and the
journey never opens the queue or expanded player. There is no minimum request
count because lazy loading, cache reuse, and coalescing can reduce it.

The preserved cap-eight diagnostic identified a ninth canonical response for the
exact owned recording during the third Playlist load's Play step. It did not
identify the specific originating image node. The ceiling follows the fixed
navigation and render slots rather than attributing that request to a node.

The browser has a separate 16 MiB, UID-owned `/var/tmp` tmpfs while its root
filesystem stays read-only. A standalone control with the exact owned WAV and
browser image failed before this mount: native GStreamer reported that its
`downloadbuffer` could not create a temporary file in `/var/tmp` on the read-only
filesystem. Changing only `HOME` did not help. Adding only this bounded tmpfs let
the same native Play call load metadata and advance. This is browser media
buffering, independent of Hito's Download API. The standalone controls and native
diagnostics are retained separately from the acceptance sources and do not
qualify the full ranking journey.

The playback check requires `HAVE_CURRENT_DATA`, matching Hito's listening
observers, together with the exact selected source, advancing media time,
unpaused state, and no media error. In WebKit the original `HAVE_FUTURE_DATA`
requirement timed out while the selected audio had already played 8,990 ms with
ready state 2. The ten-second deadline, heard-prefix bounds, PCM decoding,
progress receipts, and zero forbidden writes remain unchanged.

A separate diagnostic on the exact app `d7c7869d…` and community `19a3e426…`
artifacts read a clone of that same response to EOF: 31 canonical bytes, with one
server GET and matching request UUID. The original Playwright completion also
finished under that instrumentation. That diagnostic is retained separately and
does not qualify an uninstrumented gate.
