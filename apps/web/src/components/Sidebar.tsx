import { useState } from "react";
import { Link, router } from "@inertiajs/react";
import {
  AudioLines,
  Disc3,
  Mic2,
  Heart,
  History,
  Library,
  ListMusic,
  LogOut,
  Settings,
  Plus,
} from "lucide-react";
import { sessionApi } from "../api";
import { errorMessage } from "../format";
import { usePlayer } from "../player/PlaybackProvider";
import { useNotice } from "./Notices";
import type { Playlist, User } from "../types";
import { log } from "../log";

const navigation = [
  {
    group: "Browse",
    items: [
      { href: "/library", label: "Library", icon: Library },
      { href: "/albums", label: "Albums", icon: Disc3 },
      { href: "/artists", label: "Artists", icon: Mic2 },
      { href: "/playlists", label: "Playlists", icon: ListMusic },
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
  path,
}: {
  user: User;
  playlists: Playlist[];
  path: string;
}) {
  const player = usePlayer();
  const notice = useNotice();
  const [busy, setBusy] = useState(false);
  async function signOut() {
    setBusy(true);
    try {
      await sessionApi.signOut(user.id);
      router.clearHistory();
      player.clear();
      window.location.replace("/");
    } catch (error) {
      notice(errorMessage(error));
      setBusy(false);
    }
  }
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
                (path === "/" && href === "/library");
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
