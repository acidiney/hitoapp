Hito uses the upstream souvlaki 0.8.3 crate archive pinned in UPSTREAM.json. LICENSE is retained. This directory is excluded from workspace membership and patched explicitly from the root manifest.

The local API adds an optional opaque track object path to MediaMetadata and a SetTrackPosition event carrying that path and the original Duration. The default Linux D-Bus backend publishes that path, rejects stale/NoTrack IDs, negative or beyond-known-duration positions and unknown duration, then forwards the scoped event. Windows and macOS implementation files are unchanged; their existing unscoped position events remain supported. One example literal supplies the new optional field.

This patch does not qualify the alternate zbus backend, Windows/macOS execution, MPRIS Seeked signals, rate/volume/capability behavior, physical panels, packages or signing. Hito still validates the retained original playback binding at final command acceptance.
