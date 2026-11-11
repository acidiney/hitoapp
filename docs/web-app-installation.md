# Installable web app server contract

The server serves these exact files from the root of `HITO_WEB_DIR`. They are available through GET and HEAD without a session, including before setup.

| Paths | Content type | Successful response cache policy |
| --- | --- | --- |
| `/site.webmanifest` | `application/manifest+json` | `private, no-store` |
| `/sw.js` | `text/javascript` | `no-cache` |
| `/favicon.ico` | `image/x-icon` | `private, no-store` |
| `/favicon.svg`, `/icon.svg` | `image/svg+xml` | `private, no-store` |
| `/favicon-16.png`, `/favicon-32.png`, `/apple-touch-icon.png`, `/icon-192.png`, `/icon-512.png`, `/icon-maskable-512.png` | `image/png` | `private, no-store` |

The worker is served at `/sw.js` with `Service-Worker-Allowed: /`. Reads stay within the configured web root and stream an opened regular file. Missing files, a missing build directory, directories and outside-root symlinks return plain 404 responses. There is no general web-root file fallback or directory listing. The existing origin and security middleware still applies.

Server HTML links the manifest, ICO and SVG favicons, 16px and 32px PNG favicons, and the Apple touch icon. The viewport is exactly `width=device-width, initial-scale=1, viewport-fit=cover, interactive-widget=resizes-content`, matching the frontend safe-area and keyboard layout. It includes light `theme-color` `#fbfbfa`, dark `theme-color` `#111110`, `mobile-web-app-capable=yes`, and Apple standalone metadata with capable value `yes`, title `Hito` and status bar style `black-translucent`. Initial page data stays escaped, and the restrictive script policy is unchanged.

`/` and `/home` return `HomePage`; `/library` returns `CatalogPage`. Setup, signed-out login, PIN locks, Inertia partial reloads and asset-version conflicts keep their existing behavior.

The frontend supplies the manifest, icons, worker file and worker registration. Its worker must leave authenticated APIs, audio, artwork and signed URLs out of shared caches. The server middleware default remains `private, no-store`; routes with an existing explicit cache policy keep it.

On 2026-11-11, Docker Compose checks passed 36 focused public HTTP tests, server Clippy across all targets with warnings denied, the production library and binary check, and workspace formatting. The tests cover Linux FIFO rejection with positive cleanup of the failing reader, filesystem confinement, head metadata and escaping, authentication, Inertia behavior, and private audio and artwork responses. Browser installation and frontend worker behavior require separate acceptance. This is backend qualification, not a public release.
