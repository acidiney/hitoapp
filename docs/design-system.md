# Design system (proposed redesign)

**Status: phase 1 (foundation) implemented; later phases planned.** The tokens, bundled fonts, shared primitives, the new shell (sidebar, bottom tabs on phones) and the restyled player bar are in `apps/web`. Page contents are only restyled; the screens listed under "Screens" are not rebuilt yet. Nothing here is a public release.

| Area | State |
| --- | --- |
| Colour tokens, light and dark | Implemented in `apps/web/src/styles/base.css`; dark follows `prefers-color-scheme`. Old palette variables are aliases of the new tokens. |
| Geist and Geist Mono | Implemented, bundled locally with `@fontsource-variable` (OFL-1.1, see [third-party notices](third-party-notices.md)). No CDN. |
| Primitives in `apps/web/src/shared/ui` | Button, SegmentedControl, FilterPills, Switch, AvailabilityLabel, HealthDot, ListRow, SettingsCard, SectionWarning, FallbackCover, BrandMark. Not yet used by the old pages, which keep their class names. |
| Shell | Sidebar with the mark, Início / Pesquisar (⌘K or Ctrl+K) / Biblioteca, playlists, account link, lock and sign-out. Below 768 px the navigation becomes a three-tab bottom bar. |
| Player bar | Restyled (cover, title and artist, library ring, transport, mono times, speed, queue, expand). Below 768 px it is a mini player above the tab bar. Playback logic is unchanged. |
| Home, Biblioteca filters, Settings navigation, Contas, Artista, Álbum, Playlist, A tocar, Fila, states board | Planned (phases 2 and later). |

Route mapping in phase 1: there is no Home page yet, so Início opens `/discover`, the closest existing page, until the real Home is built. Pesquisar focuses the library search field (or opens `/library` when the page has none). Biblioteca opens `/library`; albums, artists, playlists, podcasts, audiobooks, likes and history stay at their routes and are reachable from the "Library sections" strip shown on those pages. Settings open from the avatar.

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
- No source or provider label is available for the playing track, so the player bar shows only the library ring.
