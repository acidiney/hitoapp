import { useMemo, useState, type KeyboardEvent } from "react";
import { log } from "../../../shared/log";
import { searchSettings, type SearchHit } from "../model/settingsSearch";

export function useSettingsSearch(
  admin: boolean,
  onChoose: (hit: SearchHit) => void,
) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const hits = useMemo(() => searchSettings(query, admin), [query, admin]);
  const searching = query.trim() !== "";
  const open = searching && hits.length > 0;
  const current = Math.min(active, Math.max(hits.length - 1, 0));

  function change(value: string) {
    setActive(0);
    setQuery(value);
  }

  function choose(hit: SearchHit) {
    log.debug("settings.search.choose", { mode: hit.kind });
    setQuery("");
    onChoose(hit);
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Escape" && query !== "") {
      event.preventDefault();
      event.stopPropagation();
      change("");
      return;
    }
    if (!open) return;
    const last = hits.length - 1;
    const next: Record<string, number> = {
      ArrowDown: Math.min(current + 1, last),
      ArrowUp: Math.max(current - 1, 0),
      Home: 0,
      End: last,
    };
    if (event.key in next) {
      event.preventDefault();
      setActive(next[event.key]);
    } else if (event.key === "Enter") {
      event.preventDefault();
      choose(hits[current]);
    }
  }

  return {
    query,
    hits,
    searching,
    open,
    current,
    setActive,
    change,
    choose,
    onKeyDown,
  };
}

export type SettingsSearchState = ReturnType<typeof useSettingsSearch>;
