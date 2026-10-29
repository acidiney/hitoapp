import { Link } from "@inertiajs/react";
import { Home, Library, Lock, LogOut, Plus, Search } from "lucide-react";
import { useSignOut } from "../features/session/hooks/useSignOut";
import { useLockSession } from "../features/session/hooks/useLockSession";
import { useIdleLock } from "../features/session/hooks/useIdleLock";
import { usePlayer } from "../features/playback/components/PlaybackProvider";
import BrandMark from "../shared/ui/BrandMark";
import { coverStyle } from "../shared/ui/coverPalette";
import type { LockStatus, Playlist, User } from "../shared/types";
import { log } from "../shared/log";
import { currentDestination } from "./destinations";
import { focusShellSearch } from "./LibrarySearch";

export default function Sidebar({
  user,
  playlists,
  lock,
  path,
}: {
  user: User;
  playlists: Playlist[];
  lock: LockStatus;
  path: string;
}) {
  const player = usePlayer();
  const { busy, signOut } = useSignOut(user.id, () => player.clear());
  const locking = useLockSession(user.id, () => player.clear());
  useIdleLock(user.id, lock, () => player.clear());
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
          Início
        </Link>
        <Link
          href="/library"
          onClick={(event) => {
            if (focusShellSearch()) event.preventDefault();
            else mark("/library")();
          }}
        >
          <Search size={18} aria-hidden="true" />
          Pesquisar
          <kbd>⌘K</kbd>
        </Link>
        <Link
          href="/library"
          aria-current={destination === "library" ? "page" : undefined}
          onClick={mark("/library")}
        >
          <Library size={18} aria-hidden="true" />
          Biblioteca
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
      <div className="account">
        <Link
          href="/settings"
          className="account-link"
          aria-label="Conta e definições"
          aria-current={destination === "settings" ? "page" : undefined}
          onClick={mark("/settings")}
        >
          <span className="avatar">{user.username[0]?.toUpperCase()}</span>
          <span>
            {user.username}
            <small>{user.admin ? "Administrator" : "Conta e definições"}</small>
          </span>
        </Link>
        {lock.pin_enabled && (
          <button
            className="icon-button"
            aria-label="Lock"
            disabled={locking.busy}
            onClick={() => void locking.lock()}
          >
            <Lock size={17} />
          </button>
        )}
        <button
          className="icon-button"
          aria-label="Sign out"
          disabled={busy}
          onClick={() => void signOut()}
        >
          <LogOut size={17} />
        </button>
      </div>
    </aside>
  );
}
