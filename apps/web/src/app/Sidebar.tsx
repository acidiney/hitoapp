import { Link } from "@inertiajs/react";
import { Home, Library, Plus, Search } from "lucide-react";
import BrandMark from "../shared/ui/BrandMark";
import { coverStyle } from "../shared/ui/coverPalette";
import type { Playlist } from "../shared/types";
import { log } from "../shared/log";
import { currentDestination } from "./destinations";
import { isApplePlatform, openSearch, shortcutLabel } from "./searchShortcut";

const shortcut = shortcutLabel(isApplePlatform());

export default function Sidebar({
  playlists,
  path,
}: {
  playlists: Playlist[];
  path: string;
}) {
  const destination = currentDestination(path);
  const mark = (route: string) => () =>
    log.info("navigation.requested", { route });
  return (
    <aside className="sidebar">
      <Link href="/library" className="brand">
        <BrandMark />
        <span className="brand-word">hito</span>
      </Link>
      <nav aria-label="Main navigation">
        <Link
          href="/discover"
          aria-current={destination === "home" ? "page" : undefined}
          onClick={mark("/discover")}
        >
          <Home size={18} aria-hidden="true" />
          Home
        </Link>
        <button
          type="button"
          aria-label="Open search"
          aria-keyshortcuts={shortcut.keys}
          onClick={openSearch}
        >
          <Search size={18} aria-hidden="true" />
          Search
          <kbd aria-hidden="true">{shortcut.text}</kbd>
        </button>
        <Link
          href="/library"
          aria-current={destination === "library" ? "page" : undefined}
          onClick={mark("/library")}
        >
          <Library size={18} aria-hidden="true" />
          Library
        </Link>
      </nav>
      <div className="sidebar-playlists">
        <div className="sidebar-section-heading">
          <p>Playlists</p>
          <Link href="/playlists#new-playlist" aria-label="Create a playlist">
            <Plus size={18} />
          </Link>
        </div>
        {playlists.slice(0, 12).map((playlist) => (
          <Link key={playlist.id} href={`/playlists/${playlist.id}`}>
            <span
              className="playlist-swatch"
              style={{ background: coverStyle(playlist.id).background }}
              aria-hidden="true"
            />
            <span>{playlist.name}</span>
          </Link>
        ))}
        {playlists.length === 0 && (
          <span className="muted">Keep a collection of your own.</span>
        )}
      </div>
    </aside>
  );
}
