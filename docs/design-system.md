# Design system (proposed redesign)

**Status: phase 1 (foundation), the settings half of phase 2 and the Home, Search and Discover half of phase 3 implemented; later phases planned.** The tokens, bundled fonts, shared primitives, the new shell (sidebar, bottom tabs on phones) and the restyled player bar are in `apps/web`. Page contents are only restyled except Settings, Home, Search and Discover; the other screens listed under "Screens" are not rebuilt yet. Nothing here is a public release.

| Area | State |
| --- | --- |
| Colour tokens, light and dark | Implemented in `apps/web/src/styles/base.css`; dark follows `prefers-color-scheme`. Old palette variables are aliases of the new tokens. |
| Geist and Geist Mono | Implemented, bundled locally with `@fontsource-variable` (OFL-1.1, see [third-party notices](third-party-notices.md)). No CDN. |
| Primitives in `apps/web/src/shared/ui` | Button, SegmentedControl, FilterPills, Switch, AvailabilityLabel, HealthDot, ListRow, SettingsCard, SectionWarning, FallbackCover, BrandMark. Settings use them (plus SettingRow and Dialog); the other pages keep their class names. |
| Shell | Sidebar with the mark, Home / Search (⌘K on Apple systems, Ctrl+K elsewhere) / Library, playlists, and an account bar (account and settings link, lock, sign-out) that sits at the bottom of the sidebar on desktop and in a header on phones. Below 768 px the navigation becomes a three-tab bottom bar. |
| Player bar | Restyled (cover, title and artist, transport, mono times, speed, queue, expand). Below 768 px it is a mini player above the tab bar. Playback logic is unchanged. |
| Settings | Implemented: a grouped navigation (Your account: Profile, Privacy & lock; Listening: Music & providers, Plugins, Playback, Downloads & storage; Server: Accounts, Updates, About) with one deep link per section, a section heading, and cards built from SettingsCard, SettingRow, Switch, SegmentedControl, Button, HealthDot and SectionWarning. See "Settings" below. |
| Home | Implemented at `/home` (see "Home, Search and Discover" below). |
| Search | Implemented at `/search` with All / Library / Providers scopes (see below). |
| Discover | Restyled: provider pills, section warning, availability labels. Genres, featured albums and radio are not built (see below). |
| Biblioteca filters, Artista, Álbum, Playlist, A tocar, Fila, states board | Planned (phase 2 Biblioteca and later). |

**Language.** The prototype labels are Portuguese, but the app stays English for now (Home, Search, Library, "Account and settings"). Translation is a separate i18n item in the [TODO](../TODO.md).

Routes: Home opens `/home`, Search opens `/search` (a real page, not a focus shortcut) and Library opens `/library`; albums, artists, playlists, podcasts, audiobooks, likes and history stay at their routes and are reachable from the "Library sections" strip shown on those pages. `/discover` stays a separate page reached from Home and the Providers scope of Search, and counts as part of the Search destination. Settings open from the avatar. `/` and the post-sign-in landing remain the library: several browser flows assert the library heading after sign-in, so moving the landing to Home is deferred (see the [TODO](../TODO.md)). The ⌘K / Ctrl+K shortcut focuses the field of the current page when it has one (library pages keep their own library filter, Home and Search their own field) and otherwise opens `/search` and focuses its field.

**Fallback cover rule (implemented).** `FallbackCover` takes its colours and shape from a 32-bit FNV-1a hash of the release id (the recording id when there is no release). The hash picks one of eight fixed palette entries (`coverPalette.ts`) and one of three shapes (circle, band, plain), so the same id looks the same on every visit and device. Real artwork always wins; the fallback shows only when no artwork loads.

The prototype with every screen is a private design canvas: https://claude.ai/artifact/HurdZx23txC2chX73MX529. The rows marked **v2** are the agreed direction; earlier rows are explorations kept for history.

## Principles

- **Three destinations.** Início (discover), Pesquisar (one search across library and providers, ⌘K on desktop), Biblioteca (artists, albums, playlists, podcasts and audiobooks as filters, not separate pages). Settings live behind the avatar.
- **One shell.** The sidebar, the player bar and the cover are shared components. Every screen uses them unchanged.
- **One action per state.** Each item shows its availability and only the action that state allows.
- **Nothing merges by itself.** Uncertain matches wait for review; the user decides.
- **Suggestions explain themselves.** Each suggestion says why it appears and can be dismissed.
- **Warnings stay local.** A provider or folder failure marks its own section, never the whole page.
- **Own controls.** Switches and segmented controls; no native checkboxes or selects.
- **Technical, but secondary.** Formats, conversions, durations and dates are shown in monospace and never compete with titles.

## Identity

The mark is the three availability states in a row, followed by the word "hito":

- ● filled: in the library
- ○ outlined: available from a provider
- ◌ dashed: catalog only, no audio yet

The same three rings appear wherever availability is shown, so the brand and the core product idea are the same thing.

## Colour

The table lists the base tokens. The app also defines `control-border` and `control-off` (3:1 on the page and surface, for input borders and a switch that is off), and `ink-hover` and `action-hover` for button hover. Cover colours come with an inset edge so dark covers show on a dark page.

| Token | Value | Use |
| --- | --- | --- |
| `bg` | `#fbfbfa` | Page |
| `surface` | `#ffffff` | Cards, player |
| `subtle` | `#eeeeea` | Selection, segmented controls |
| `line` | `#e6e5e1` | Dividers |
| `ink` | `#141414` | Text, primary action |
| `muted` | `#5f5e5a` | Metadata (6.3:1 on `bg`) |
| `action` | `#2f4bff` | Add, available |
| `success` | `#0f7a4f` | Working |
| `warning` | `#9a5b00` | Slow, disconnected |
| `error` | `#b5412b` | Failures, destructive text |

Dark mode uses the same tokens with `bg #111110`, `surface #1b1b19`, `ink #f2f1ed`, `muted #a3a29c` and `action #6f86ff`. All body text meets 4.5:1.

**Cover colour.** Artist, album, podcast and playlist headers use one flat colour taken from the cover, never a gradient. With real artwork, the server computes the dominant colour once when the artwork is stored and keeps it with the artwork. Without artwork, the colour and fallback cover shape come from a stable hash of the release id over a fixed palette, so they never change between visits or devices.

## Typography

Geist for the interface, Geist Mono for technical values.

| Role | Size / weight | Example |
| --- | --- | --- |
| Hero | 64–88 / 800, −0.05em | Artist name |
| Page title | 36 / 800, −0.04em | Definições |
| Section | 22 / 800, −0.02em | Saiu dos teus artistas |
| Item | 15 / 700 | Track title |
| Body | 15 / 400 | Descriptions |
| Label | 12–13 / 700 | Availability, status |
| Technical | Geist Mono 12 | `FLAC 24/384 → 48 kHz · 1,25×` |

## Components

- **Buttons**, 44 px high, radius 10–12: primary (ink), action (blue, add/import), secondary (outline), destructive (outline with red text, never filled), active (outline with a check). One primary per zone.
- **Segmented control** for two to five exclusive options (lock timeout, speed, update channel). **Pills** for filters. **Switch** for on/off settings with immediate effect.
- **Availability label**: ring plus text, using the three states above. **Health dot**: filled dot in success, warning or action colour for services, folders and plugins.
- **List row**: cover (44–64 px), title and subtitle, state, one action. Fixed column widths so states and actions align.
- **Settings card**: header with title and one action, then rows; a shared settings navigation lists Perfil, Privacidade e bloqueio, Música e fornecedores, Plugins, Reprodução, Downloads e espaço, Contas, Atualizações and Sobre.
- **Section warning**: amber or red box inside the affected section, saying what happened, what still works and what to do.
- **Fallback cover**: typographic cover (title, artist, one shape) used only when there is no real artwork.

## Screens

Desktop screens in the prototype: Início, Artista, Álbum, Biblioteca, Playlist (view and edit), Descobrir, Podcast, Audiolivro, A tocar (full screen with Up next, Lyrics and Source tabs), Fila, Definições (all sections), Contas with invite creation, Entrar, Criar conta, and a board of real states (loading, provider down, offline, empty library, import problems, unplayable track). Phone screens exist for Pesquisa and A tocar.

Account creation is never linked from the sign-in screen. It is reachable only through a single-use invite link, or on the first start of a server with no accounts, where it creates the administrator. The screen infers which case applies.

## Still open

- Phone versions of the remaining screens.
- Dark mode screens.
- Rebuilding each screen with the shared components (phases 2 and later); the speed control is still a native select and the player bar has no queue count.
- The cover colour for headers (computed from real artwork on the server) is not built.
- No source or provider label is available for the playing track, so the player bar shows none until the track carries a source.

## Settings

Current behavior (apps/web, English labels):

- `/settings?section=<id>` opens a section: `profile`, `privacy`, `music`, `plugins`, `playback`, `downloads`, `accounts`, `updates`, `about`. With no parameter the page opens Music & providers on desktop and the section list on phones. Old links keep working: `library` and `metadata` open Music & providers; an unknown id, or `accounts` for a non-administrator, opens Music & providers. Accounts is hidden from the navigation for non-administrators.
- The navigation is a list of links (`aria-current="page"` marks the open section). Choosing a section moves focus to its heading. Below 768 px the list and the open section are separate views: the list, then the section with an "All settings" link back. The phase 1 account bar is hidden on phone Settings; Profile offers Sign out and, when a PIN is set, Lock now.
- Rows use fixed status and action columns (200 px and 140 px; 160 px and 300 px for rows with several buttons) so status and buttons line up between rows. On phones the columns stack.
- On/off choices are Switches and the idle-lock period is a segmented control. Switches in Privacy keep explicit Save and Cancel, because consent changes must be saved deliberately. Three native selects remain on purpose, for lists whose length depends on the server: the library chooser, the service chooser when connecting a plugin and the metadata provider chooser. The folder picker keeps its own dialog markup and has not moved to the shared Dialog yet.
- Leaving a section with unsaved changes (community choices, source rankings, metadata preferences, a typed PIN or password) asks for confirmation first. A member or alias URL such as `?section=library` is replaced by the resolved section in the address bar.
- On phones, Sign out and Lock now are available only in Profile, because the account bar is hidden on Settings.
- Creating an account opens a dialog from the Accounts card. The server has no invitation or public sign-up flow, and the interface offers none.

Not built because the server has no matching capability (see the [TODO](../TODO.md)): display name, avatar photo, language, password change, open-session list, playback-history toggle and deletion, update checks and channels, copy diagnostics, playback and download options.

Where the old pages were: Library settings now live in Music & providers (music folders); Metadata moved into the same section; Privacy keeps community consent and source rankings and gains the screen lock; Plugins and Accounts keep their content.

## Home, Search and Discover

**Home (`/home`).** A greeting with the account name and a search field that sends the query to Search, then only sections the server can fill from data it already has:

- *Quick access*, up to six tiles: the three most recently played albums, Liked tracks (with a count, shown as `1,000+` above 1,000), then the account's own and followed playlists, then any further recent albums. Each tile opens its page.
- *Unheard, near what you like*, up to three albums the account has played where other playable tracks have never been started. Each card says why ("You played 3 tracks from Tide.") and offers Listen (queues those tracks) and Open album. There is no "Not now" because nothing persists a dismissal.
- An empty library (no music, no playlists) shows folder, playlist-import and service actions; a listener who cannot manage folders is told to ask the administrator.

"New from your artists" is not shown: nothing in the server polls provider discographies. The read model is `Collection::home` in core (recent albums come from the 200 most recent progress rows, so its work does not grow with listening history; a SQLite work-budget test covers this) and the thin `HomePage` adapter in the server.

**Search (`/search`).** One field and the pills All / Library / Providers. Library results come from the existing catalog search and are grouped as Top result, Tracks (availability label: In library or Catalog only; Play or View), Albums and Artists, at most 8 tracks and 6 albums and artists. Provider results use the first connected service that can search (a pill group chooses another) through the existing provider search, with the same Listen and Add actions as Discover and a section warning when that service fails, while the library results stay. The scope is client state and is not in the URL.

**Discover (`/discover`).** The provider search page, restyled: provider filter pills, the search field, availability labels (In library, Available, Catalog only), Listen (play) and Add actions and an inline section warning with a service refresh link on a provider failure. By-genre tiles, featured albums per provider and "Radio from…" are not built because no plugin capability supplies them.

