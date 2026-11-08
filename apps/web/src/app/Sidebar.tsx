import type { Schema } from "../shared/schema";
import { Link } from "@inertiajs/react";
import { Compass, Home, Library, Plus, Search } from "lucide-react";
import BrandMark from "../shared/ui/BrandMark";
import CoverArtSvg from "../shared/ui/CoverArtSvg";
import { coverArt } from "../shared/ui/coverArt";

import { log } from "../shared/log";
import { currentDestination } from "./destinations";
import { isApplePlatform, shortcutLabel } from "./searchShortcut";

const shortcut = shortcutLabel(isApplePlatform());

export default function Sidebar({
  playlists,
  path,
}: {
  playlists: Schema<"Playlist">[];
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
          href="/home"
          aria-current={destination === "home" ? "page" : undefined}
          onClick={mark("/home")}
        >
          <Home size={18} aria-hidden="true" />
          Home
        </Link>
        <Link
          href="/search"
          aria-keyshortcuts={shortcut.keys}
          aria-current={destination === "search" ? "page" : undefined}
          onClick={mark("/search")}
        >
          <Search size={18} aria-hidden="true" />
          Search
          <kbd aria-hidden="true">{shortcut.text}</kbd>
        </Link>
        <Link
          href="/discover"
          aria-current={destination === "discover" ? "page" : undefined}
          onClick={mark("/discover")}
        >
          <Compass size={18} aria-hidden="true" />
          Discover
        </Link>
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
            <span className="playlist-swatch" aria-hidden="true">
              <CoverArtSvg art={coverArt("playlist", playlist.id, "", null)} />
            </span>
            <span title={playlist.name}>{playlist.name}</span>
          </Link>
        ))}
        {playlists.length === 0 && (
          <span className="muted">Keep a collection of your own.</span>
        )}
      </div>
    </aside>
  );
}
