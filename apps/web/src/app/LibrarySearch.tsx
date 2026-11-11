import { useEffect, useState, type FormEvent } from "react";
import { router } from "@inertiajs/react";
import { Search } from "lucide-react";
import { log } from "../shared/log";
import type { PluginCounts } from "../features/plugins/model/pluginCounts";
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
  plugins = null,
}: {
  query: string;
  target?: "library" | "albums" | "artists" | "search";
  plugins?: PluginCounts | null;
}) {
  const [search, setSearch] = useState(query);
  useEffect(() => setSearch(query), [query]);
  function submit(event: FormEvent) {
    event.preventDefault();
    log.info("navigation.search", { enabled: Boolean(search) });
    const parameters: Record<string, string> = {};
    if (search) parameters.q = search;
    const sort = new URLSearchParams(window.location.search).get("sort");
    if (sort && target !== "search") parameters.sort = sort;
    router.get(`/${target}`, parameters);
  }
  const global = target === "search";
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
      </form>
      <PluginsPill counts={plugins} />
    </header>
  );
}
