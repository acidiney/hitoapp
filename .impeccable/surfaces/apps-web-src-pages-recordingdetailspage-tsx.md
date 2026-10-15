---
version: 1
slug: "apps-web-src-pages-recordingdetailspage-tsx"
primary_target: "apps/web/src/pages/RecordingDetailsPage.tsx"
related_targets: ["apps/web/src/components/SourceAvailability.tsx","apps/web/src/hooks/useSourceAvailability.ts"]
---

Operate: a listener rechecks audio for a saved provider track and starts playback only when they choose Play.

Extend the existing track-details page with one inline Audio availability section between personal fields and source observations. Keep the current Hito typography, spacing, controls and charcoal/orange visual system. Reuse the persistent player. Show real pending, ready, unavailable and service-error states; keep Check again in the same position and link to music service settings when recovery needs attention.

A check updates source availability without changing accepted metadata, private edits or the current audio. No automatic provider request runs merely from viewing details. Account and recording changes discard old results and abort requests. The user can retry a previously unavailable provider track without entering environment variables, remote IDs or service addresses.

Keep the section readable on phones above the dock, with wrapping controls, visible keyboard focus and announced status. Preserve draft metadata and ongoing playback while checking. No new artwork, fabricated albums, decorative card, modal, statistics or brand direction is introduced.
