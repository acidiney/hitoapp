# Hito interface

The user's music-app references are the visual authority. They show charcoal and black workspaces, orange controls, cover-led content, grouped navigation, a persistent bottom player, a substantial queue, and clearly grouped settings. Replace the initial assumed light-blue interface.

The composition leads with the collection rather than an administration table: a real recording spotlight and playlist shortcuts above an artwork grid, with a list view available for dense browsing. Group Browse, Your music, and Manage navigation in the left rail. A persistent right listening panel contains artwork, current metadata, and upcoming tracks; it collapses on smaller screens. The footer survives navigation. Expanded playback places large artwork beside the queue, with transport controls below. Settings use real tabs and clear rows. Artwork comes from authorized embedded tags or neighboring cover files; absent artwork stays neutral. These are layout commitments, not a palette-only treatment.

## Visual system

Use a dark canvas #101010, inset workspace #151515, raised panels #1e1e1e, hover surfaces #282828, white primary text #f5f5f5, secondary text #aaaaaa, and orange #ff7848. Use orange for active navigation, progress, selected states, and primary actions. Artwork may carry other colors. Keep separators subtle at #2b2b2b.

Use a system sans-serif stack, clear medium-weight headings, compact supporting text, and tabular playback times. The shell has a grouped left navigation rail, a wide search control, a main content column, and a bottom player. Keep the interface useful at laptop sizes and collapse navigation/secondary columns on phones.

## Content and interaction

Artwork should come from actual library/provider metadata when available. Missing artwork gets a deliberate neutral fallback. Do not populate the product with invented tracks, artists, listening statistics, social features, or subscriptions. The supplied screenshots are references, not Hito library data or assets to relabel.

A populated library should give music visual priority. Empty states should still have a composed hierarchy and a clear first action. Settings use section navigation and clear rows. Queue and player controls remain accessible and stable across route changes.

## Quality

Every interactive element has focus, hover, loading, disabled, empty, and failure states where relevant. Normal text meets 4.5:1 contrast; controls have comfortable hit targets and visible labels or accessible names. Honor reduced motion. Verify desktop and phone layouts in the Docker browser before accepting the surface.
