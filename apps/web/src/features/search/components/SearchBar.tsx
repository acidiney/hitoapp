import { useState } from "react";
import type { ReactNode } from "react";
import { Search, X } from "lucide-react";
import { SHELL_SEARCH_ID } from "../../../app/searchShortcut";
import PlaylistLinkImport, {
  PlaylistLinkHint,
} from "../../playlists/components/PlaylistLinkImport";
import { looksLikePlaylistLink } from "../../playlists/model/playlistService";

export default function SearchBar({
  value,
  onChange,
  onSubmit,
}: {
  value: string;
  onChange: (value: string) => void;
  onSubmit: (value: string) => void;
}) {
  const [linkDialog, setLinkDialog] = useState<string | null>(null);
  const playlistLink = looksLikePlaylistLink(value);
  return (
    <>
      <form
        className="search-bar"
        role="search"
        onSubmit={(event) => {
          event.preventDefault();
          if (playlistLink) setLinkDialog(value.trim());
          else onSubmit(value);
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
        {playlistLink && (
          <PlaylistLinkHint
            link={value}
            onOpen={() => setLinkDialog(value.trim())}
          />
        )}
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
      {linkDialog !== null && (
        <PlaylistLinkImport
          link={linkDialog}
          onClose={() => setLinkDialog(null)}
        />
      )}
    </>
  );
}

export function SearchHints({ children }: { children?: ReactNode }) {
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
      {children}
    </p>
  );
}
