import { Link } from "@inertiajs/react";
import {
  AudioLines,
  Disc3,
  Mic2,
  Heart,
  History,
  Library,
  Compass,
  ListMusic,
  Lock,
  LogOut,
  Settings,
  Plus,
  Podcast,
  BookOpen,
} from "lucide-react";
import { useSignOut } from "../features/session/hooks/useSignOut";
import { useLockSession } from "../features/session/hooks/useLockSession";
import { useIdleLock } from "../features/session/hooks/useIdleLock";
import { usePlayer } from "../features/playback/components/PlaybackProvider";
import type { LockStatus, Playlist, User } from "../shared/types";
import { log } from "../shared/log";

const navigation = [
  {
    group: "Browse",
    items: [
      { href: "/library", label: "Library", icon: Library },
      { href: "/discover", label: "Discover", icon: Compass },
      { href: "/albums", label: "Albums", icon: Disc3 },
      { href: "/artists", label: "Artists", icon: Mic2 },
      { href: "/playlists", label: "Playlists", icon: ListMusic },
      { href: "/audiobooks", label: "Audiobooks", icon: BookOpen },
      { href: "/podcasts", label: "Podcasts", icon: Podcast },
    ],
  },
  {
    group: "Your music",
    items: [
      { href: "/likes", label: "Liked tracks", icon: Heart },
      { href: "/history", label: "History", icon: History },
    ],
  },
  {
    group: "Manage",
    items: [{ href: "/settings", label: "Settings", icon: Settings }],
  },
];

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
  return (
    <aside className="sidebar">
      <Link href="/library" className="brand">
        <AudioLines size={31} strokeWidth={2.5} />
        hito<span>audio, together</span>
      </Link>
      <nav aria-label="Main navigation">
        {navigation.map(({ group, items }) => (
          <div className="nav-group" key={group}>
            <span className="nav-group-label">{group}</span>
            {items.map(({ href, label, icon: Icon }) => {
              const selected =
                path === href ||
                path.startsWith(`${href}/`) ||
                (href === "/artists" && path.startsWith("/artist-groups/")) ||
                ((path === "/" || path.startsWith("/recordings/")) &&
                  href === "/library");
              return (
                <Link
                  key={href}
                  href={href}
                  className={selected ? "active" : ""}
                  aria-current={selected ? "page" : undefined}
                  onClick={() =>
                    log.info("navigation.requested", { route: href })
                  }
                >
                  <Icon size={19} />
                  {label}
                </Link>
              );
            })}
          </div>
        ))}
      </nav>
      <div className="sidebar-playlists">
        <div className="sidebar-section-heading">
          <p>Your playlists</p>
          <Link href="/playlists#new-playlist" aria-label="Create a playlist">
            <Plus size={17} />
          </Link>
        </div>
        {playlists.slice(0, 12).map((playlist) => (
          <Link key={playlist.id} href={`/playlists/${playlist.id}`}>
            <ListMusic size={16} />
            <span>{playlist.name}</span>
          </Link>
        ))}
        {playlists.length === 0 && (
          <span className="muted">Keep a collection of your own.</span>
        )}
      </div>
      <div className="account">
        <span className="avatar">{user.username[0]?.toUpperCase()}</span>
        <span>
          {user.username}
          <small>{user.admin ? "Administrator" : "Listener"}</small>
        </span>
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
