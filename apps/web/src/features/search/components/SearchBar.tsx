import { Search, X } from "lucide-react";
import { SHELL_SEARCH_ID } from "../../../app/searchShortcut";

export default function SearchBar({
  value,
  onChange,
  onSubmit,
}: {
  value: string;
  onChange: (value: string) => void;
  onSubmit: (value: string) => void;
}) {
  return (
    <form
      className="search-bar"
      role="search"
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit(value);
      }}
    >
      <label>
        <Search size={20} aria-hidden="true" />
        <span className="sr-only">Search</span>
        <input
          id={SHELL_SEARCH_ID}
          type="search"
          placeholder="Search, or paste a playlist link"
          value={value}
          maxLength={512}
          autoComplete="off"
          onChange={(event) => onChange(event.target.value)}
        />
      </label>
      {value && (
        <button
          type="button"
          className="search-bar-clear"
          aria-label="Clear search"
          onClick={() => onSubmit("")}
        >
          <X size={18} aria-hidden="true" />
        </button>
      )}
    </form>
  );
}

export function SearchHints() {
  const apple = /Mac|iPhone|iPad/.test(
    typeof navigator === "undefined" ? "" : navigator.platform,
  );
  return (
    <p className="search-hints">
      <span>
        <kbd>↑</kbd> <kbd>↓</kbd> move
      </span>
      <span>
        <kbd>↵</kbd> play
      </span>
      <span>
        <kbd>{apple ? "⌘↵" : "Ctrl ↵"}</kbd> add to queue
      </span>
      <span>Results update as you type</span>
    </p>
  );
}
