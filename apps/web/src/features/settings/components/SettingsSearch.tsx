import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { Search, X } from "lucide-react";
import { log } from "../../../shared/log";
import {
  isSettingsSearchShortcut,
  searchSettings,
  type SearchHit,
} from "../model/settingsSearch";

function useSearchShortcut(field: React.RefObject<HTMLInputElement | null>) {
  useEffect(() => {
    function onKeyDown(event: globalThis.KeyboardEvent) {
      const input = field.current;
      if (!input || !isSettingsSearchShortcut(event)) return;
      if (document.querySelector("dialog[open], [aria-modal='true']")) return;
      if (input.checkVisibility && !input.checkVisibility()) return;
      event.preventDefault();
      input.focus();
      input.select();
    }
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [field]);
}

export default function SettingsSearch({
  admin,
  query,
  onQuery,
  onChoose,
}: {
  admin: boolean;
  query: string;
  onQuery: (query: string) => void;
  onChoose: (hit: SearchHit) => void;
}) {
  const id = useId();
  const listId = `${id}-results`;
  const field = useRef<HTMLInputElement>(null);
  const [active, setActive] = useState(0);
  const hits = useMemo(() => searchSettings(query, admin), [query, admin]);
  const searching = query.trim() !== "";
  const open = searching && hits.length > 0;
  const current = Math.min(active, Math.max(hits.length - 1, 0));
  useSearchShortcut(field);

  function change(value: string) {
    setActive(0);
    onQuery(value);
  }

  function choose(hit: SearchHit) {
    log.debug("settings.search.choose", { mode: hit.kind });
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

  return (
    <div className="settings-search">
      <div className="search-field" role="search">
        <label>
          <span className="sr-only">Search settings</span>
          <Search size={17} aria-hidden="true" />
          <input
            ref={field}
            type="text"
            role="combobox"
            autoComplete="off"
            spellCheck={false}
            maxLength={120}
            placeholder="Search settings"
            aria-label="Search settings"
            aria-expanded={open}
            aria-controls={open ? listId : undefined}
            aria-autocomplete="list"
            aria-activedescendant={open ? `${id}-hit-${current}` : undefined}
            value={query}
            onChange={(event) => change(event.target.value)}
            onKeyDown={onKeyDown}
          />
          {searching && (
            <button
              type="button"
              className="icon-button"
              aria-label="Clear search"
              onClick={() => {
                change("");
                field.current?.focus();
              }}
            >
              <X size={16} aria-hidden="true" />
            </button>
          )}
        </label>
      </div>
      {open && (
        <ul
          id={listId}
          role="listbox"
          aria-label="Matching settings"
          className="settings-results"
        >
          {hits.map((hit, index) => (
            <li
              key={hit.key}
              id={`${id}-hit-${index}`}
              role="option"
              aria-selected={index === current}
              data-active={index === current}
              onMouseDown={(event) => event.preventDefault()}
              onMouseMove={() => setActive(index)}
              onClick={() => choose(hit)}
            >
              <span className="settings-result-label">{hit.label}</span>
              <span className="settings-result-place">
                {hit.kind === "section" ? "Section" : hit.sectionLabel}
              </span>
            </li>
          ))}
        </ul>
      )}
      {searching && !open && (
        <p className="settings-no-results" role="status">
          No settings match “{query.trim()}”.
        </p>
      )}
      {open && (
        <p className="sr-only" role="status">
          {hits.length} {hits.length === 1 ? "result" : "results"}
        </p>
      )}
    </div>
  );
}
