import { useEffect, useId, useRef } from "react";
import { ChevronRight, Search, X } from "lucide-react";
import { isSettingsSearchShortcut } from "../model/settingsSearch";
import type { SettingsSearchState } from "../hooks/useSettingsSearch";

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

export function SettingsSearchField({
  search,
  listId,
}: {
  search: SettingsSearchState;
  listId: string;
}) {
  const field = useRef<HTMLInputElement>(null);
  useSearchShortcut(field);
  return (
    <div className="settings-search" role="search">
      <label>
        <span className="sr-only">Search settings</span>
        <Search size={18} aria-hidden="true" />
        <input
          ref={field}
          type="text"
          role="combobox"
          autoComplete="off"
          spellCheck={false}
          maxLength={120}
          placeholder="Search settings"
          aria-label="Search settings"
          aria-expanded={search.open}
          aria-controls={search.open ? listId : undefined}
          aria-autocomplete="list"
          aria-activedescendant={
            search.open ? `${listId}-hit-${search.current}` : undefined
          }
          value={search.query}
          onChange={(event) => search.change(event.target.value)}
          onKeyDown={search.onKeyDown}
        />
        {search.searching && (
          <button
            type="button"
            className="icon-button"
            aria-label="Clear search"
            onClick={() => {
              search.change("");
              field.current?.focus();
            }}
          >
            <X size={16} aria-hidden="true" />
          </button>
        )}
      </label>
    </div>
  );
}

export function SettingsSearchResults({
  search,
  listId,
}: {
  search: SettingsSearchState;
  listId: string;
}) {
  const { hits, open, current, query } = search;
  return (
    <div className="settings-results-panel">
      {open && (
        <>
          <p className="settings-results-count" role="status">
            {hits.length}{" "}
            {hits.length === 1 ? "setting matches" : "settings match"} “
            {query.trim()}”
          </p>
          <ul
            id={listId}
            role="listbox"
            aria-label="Matching settings"
            className="settings-results"
          >
            {hits.map((hit, index) => (
              <li
                key={hit.key}
                id={`${listId}-hit-${index}`}
                role="option"
                aria-selected={index === current}
                data-active={index === current}
                onMouseDown={(event) => event.preventDefault()}
                onMouseMove={() => search.setActive(index)}
                onClick={() => search.choose(hit)}
              >
                <span className="settings-result-copy">
                  <span className="settings-result-label">{hit.label}</span>
                  <span className="settings-result-place">{hit.place}</span>
                </span>
                <ChevronRight size={16} aria-hidden="true" />
              </li>
            ))}
          </ul>
        </>
      )}
      {!open && (
        <p className="settings-no-results" role="status">
          Nothing in Settings matches “{query.trim()}”.
        </p>
      )}
    </div>
  );
}

export function useSearchListId(): string {
  return `${useId()}-results`;
}
