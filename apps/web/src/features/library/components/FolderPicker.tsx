import type { Schema } from "../../../shared/schema";
import { useEffect, useRef } from "react";
import { ChevronRight, Folder, X } from "lucide-react";
import { useDirectoryBrowser } from "../hooks/useDirectoryBrowser";

import Button from "../../../shared/ui/Button";

type Props = {
  account: string;
  pending: boolean;
  error: string;
  onSelect: (directory: Schema<"DirectoryRef">) => Promise<boolean>;
  onClose: () => void;
};

export default function FolderPicker({
  account,
  pending,
  error,
  onSelect,
  onClose,
}: Props) {
  const dialog = useRef<HTMLDialogElement>(null);
  const browser = useDirectoryBrowser(account);
  const selected = browser.selection;
  const components = selected?.components ?? [];
  const grant = browser.directories.find(
    (directory) => directory.id === selected?.directory_id,
  );
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);
  async function choose() {
    if (selected && (await onSelect(selected))) onClose();
  }
  return (
    <dialog
      ref={dialog}
      className="folder-picker"
      aria-labelledby="folder-picker-title"
      aria-describedby="folder-picker-description"
      onCancel={(event) => {
        event.preventDefault();
        if (!pending) onClose();
      }}
    >
      <header>
        <div>
          <h2 id="folder-picker-title">Add a music folder</h2>
          <p id="folder-picker-description">
            Choose a folder shared with this Hito server.
          </p>
        </div>
        <button
          className="icon-button"
          aria-label="Close folder browser"
          disabled={pending}
          onClick={onClose}
        >
          <X size={20} />
        </button>
      </header>
      <nav className="folder-breadcrumbs" aria-label="Folder location">
        <button
          disabled={pending || !selected}
          onClick={() => browser.select(null)}
        >
          Shared folders
        </button>
        {selected && (
          <>
            <ChevronRight size={15} aria-hidden="true" />
            <button
              disabled={pending || components.length === 0}
              onClick={() => browser.select({ ...selected, components: [] })}
            >
              {grant?.name ?? "Shared folder"}
            </button>
            {components.map((part, index) => (
              <span key={index}>
                <ChevronRight size={15} aria-hidden="true" />
                <button
                  disabled={pending || index === components.length - 1}
                  onClick={() =>
                    browser.select({
                      ...selected,
                      components: components.slice(0, index + 1),
                    })
                  }
                >
                  {part}
                </button>
              </span>
            ))}
          </>
        )}
      </nav>
      <div className="folder-browser-content" aria-busy={browser.loading}>
        {!selected ? (
          <ul className="folder-browser-list">
            {browser.directories.map((directory) => (
              <li key={directory.id}>
                <button
                  disabled={pending || !directory.available}
                  aria-label={`Open ${directory.name}`}
                  onClick={() =>
                    browser.select({
                      directory_id: directory.id,
                      components: [],
                    })
                  }
                >
                  <Folder size={23} />
                  <span>
                    {directory.name}
                    <small>
                      {directory.available
                        ? "Browse folders"
                        : "Currently unavailable"}
                    </small>
                  </span>
                  <ChevronRight size={18} />
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <ul className="folder-browser-list">
            {browser.page?.directories.map((entry) => (
              <li key={entry.name}>
                <button
                  disabled={pending}
                  aria-label={`Open ${entry.name}`}
                  onClick={() => browser.select(entry.directory)}
                >
                  <Folder size={21} />
                  <span>{entry.name}</span>
                  <ChevronRight size={18} />
                </button>
              </li>
            ))}
          </ul>
        )}
        {browser.loading && <p role="status">Reading folders…</p>}
        {!browser.loading &&
          !browser.error &&
          (selected
            ? browser.page?.directories.length === 0
            : browser.directories.length === 0) && (
            <p>
              {selected
                ? "This folder has no subfolders."
                : "No folders have been shared with this server yet."}
            </p>
          )}
        {browser.page?.next_offset != null && (
          <Button
            disabled={browser.loading || pending}
            onClick={() => void browser.more()}
          >
            Show more folders
          </Button>
        )}
        {browser.error && (
          <div role="alert">
            <p className="form-error">{browser.error}</p>
            <button className="text-link" onClick={browser.retry}>
              Try again
            </button>
          </div>
        )}
      </div>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      <footer>
        <p>Your originals stay in place. Hito only reads them.</p>
        <Button
          variant="primary"
          disabled={
            pending ||
            browser.loading ||
            !!browser.error ||
            !browser.page ||
            !selected
          }
          onClick={() => void choose()}
        >
          {pending ? "Adding folder…" : "Use this folder"}
        </Button>
      </footer>
    </dialog>
  );
}
