---
version: 1
slug: "apps-web-src-pages-artistpage-tsx"
primary_target: "apps/web/src/pages/ArtistPage.tsx"
related_targets: ["apps/web/src/pages/ArtistIndexPage.tsx","apps/web/src/components/Pagination.tsx"]
---

Operate: a listener opens an exact artist credit to find and play saved recordings and album appearances.

Extend the existing dark Hito collection pages. Keep the artist heading, authorized artwork or neutral fallback, album covers, persistent listening panel and player. Add a dense track section with the existing playback, likes, playlist, and details controls. Artist counts describe real selected recording credits and source/release appearances; do not fabricate albums or covers for albumless provider tracks.

Keep album and recording pagination independent in the URL. One page per section is rendered; playback controls explicitly say this page when the track collection exceeds the page size. Keep unavailable saved tracks visible, private artist fields account-owned, and the page free of playback-position subscriptions.

On phones the existing responsive track rows and section rhythm remain readable above the player. Empty, out-of-range, disabled, and navigation states explain the next action without claiming a recording belongs to an invented release. Search and account boundaries remain those of the existing catalog.
