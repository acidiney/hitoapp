import { useEffect, useState, type FormEvent } from "react";
import { router } from "@inertiajs/react";
import { Search } from "lucide-react";
import { log } from "../shared/log";
import PlaylistLinkImport, {
  PlaylistLinkHint,
} from "../features/playlists/components/PlaylistLinkImport";
import { looksLikePlaylistLink } from "../features/playlists/model/playlistService";
import PluginsPill from "./PluginsPill";
import {
  SHELL_SEARCH_ID,
  isApplePlatform,
  shortcutLabel,
} from "./searchShortcut";

const shortcut = shortcutLabel(isApplePlatform());

export default function LibrarySearch({
  query,
  target = "library",
}: {
  query: string;
  target?: "library" | "albums" | "artists" | "search";
}) {
  const [search, setSearch] = useState(query);
  const [linkDialog, setLinkDialog] = useState<string | null>(null);
  useEffect(() => setSearch(query), [query]);
  const global = target === "search";
  const playlistLink = global && looksLikePlaylistLink(search);
  function submit(event: FormEvent) {
    event.preventDefault();
    if (playlistLink) {
      setLinkDialog(search.trim());
      return;
    }
    log.info("navigation.search", { enabled: Boolean(search) });
    const parameters: Record<string, string> = {};
    if (search) parameters.q = search;
    const sort = new URLSearchParams(window.location.search).get("sort");
    if (sort && target !== "search") parameters.sort = sort;
    router.get(`/${target}`, parameters);
  }
  return (
    <header className="topbar">
      <form className="search" onSubmit={submit}>
        <Search size={18} aria-hidden="true" />
        <input
          id={SHELL_SEARCH_ID}
          aria-label={global ? "Search" : `Search ${target}`}
          aria-keyshortcuts={shortcut.keys}
          placeholder={
            global
              ? "Search, or paste a playlist link"
              : `Search your ${target}`
          }
          value={search}
          onChange={(event) => setSearch(event.target.value)}
        />
        <kbd aria-hidden="true">{shortcut.text}</kbd>
        <button type="submit" className="sr-only">
          Search
        </button>
        {playlistLink && (
          <PlaylistLinkHint
            link={search}
            onOpen={() => setLinkDialog(search.trim())}
          />
        )}
      </form>
      <PluginsPill />
      {linkDialog !== null && (
        <PlaylistLinkImport
          link={linkDialog}
          onClose={() => setLinkDialog(null)}
        />
      )}
    </header>
  );
}
