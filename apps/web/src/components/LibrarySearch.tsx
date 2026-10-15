import { useEffect, useState, type FormEvent } from "react";
import { router } from "@inertiajs/react";
import { Search } from "lucide-react";
import { log } from "../log";

export default function LibrarySearch({ query }: { query: string }) {
  const [search, setSearch] = useState(query);
  useEffect(() => setSearch(query), [query]);
  function submit(event: FormEvent) {
    event.preventDefault();
    log.info("navigation.search", { enabled: Boolean(search) });
    router.get("/library", search ? { q: search } : {});
  }
  return (
    <header className="topbar">
      <form className="search" onSubmit={submit}>
        <Search size={19} />
        <input
          aria-label="Search library"
          placeholder="Search your library"
          value={search}
          onChange={(event) => setSearch(event.target.value)}
        />
        <button type="submit">Search</button>
      </form>
      <span className="server-label">Self-hosted library</span>
    </header>
  );
}
