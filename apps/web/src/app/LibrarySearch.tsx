import { useEffect, useState, type FormEvent } from "react";
import { router } from "@inertiajs/react";
import { Search } from "lucide-react";
import { log } from "../shared/log";
import { SHELL_SEARCH_ID } from "./searchShortcut";

export default function LibrarySearch({
  query,
  target = "library",
}: {
  query: string;
  target?: "library" | "albums" | "artists";
}) {
  const [search, setSearch] = useState(query);
  useEffect(() => setSearch(query), [query]);
  function submit(event: FormEvent) {
    event.preventDefault();
    log.info("navigation.search", { enabled: Boolean(search) });
    router.get(`/${target}`, search ? { q: search } : {});
  }
  return (
    <header className="topbar">
      <form className="search" onSubmit={submit}>
        <Search size={19} />
        <input
          id={SHELL_SEARCH_ID}
          aria-label={`Search ${target}`}
          placeholder={`Search your ${target}`}
          value={search}
          onChange={(event) => setSearch(event.target.value)}
        />
        <button type="submit">Search</button>
      </form>
      <span className="server-label">Self-hosted library</span>
    </header>
  );
}
