import ActionMenu, { type ActionMenuItem } from "../../../shared/ui/ActionMenu";
import type { Schema } from "../../../shared/schema";
import { createContext, useContext, type ReactNode } from "react";
import { Download } from "lucide-react";

import Button from "../../../shared/ui/Button";
import { usePlaylistDownload } from "../hooks/usePlaylistDownload";

export type PlaylistDownloadState = {
  download: Schema<"PlaylistDownloadStatus"> | null;
  checking: boolean;
  pending: boolean;
  requested: boolean;
  hasRetainedFailures: boolean;
  retryKind: "create" | "remove" | null;
  error: string;
  statusError: string;
  start(): Promise<void>;
  remove(): Promise<void>;
  retryFailed(): Promise<void>;
  retry(): Promise<void>;
  check(): Promise<void>;
};

const DownloadContext = createContext<PlaylistDownloadState | null>(null);

function useDownloadState() {
  const state = useContext(DownloadContext);
  if (!state) throw new Error("Playlist download state is not provided.");
  return state;
}

export function PlaylistDownloadProvider({
  account,
  playlist,
  children,
}: {
  account: string;
  playlist: string;
  children: ReactNode;
}) {
  const state = usePlaylistDownload(account, playlist);
  return (
    <DownloadContext.Provider value={state}>
      {children}
    </DownloadContext.Provider>
  );
}

function summary(download: PlaylistDownloadState["download"]) {
  const entries = download?.entries.items ?? [];
  const count = (state: string) =>
    entries.filter((entry) => entry.status === state).length;
  return {
    entries,
    ready: count("ready"),
    waiting: count("waiting_for_source"),
    lowStorage: entries.filter(
      (entry) =>
        entry.status === "queued" && entry.error === "insufficient_storage",
    ).length,
    failed: count("failed"),
    unavailable: count("unavailable"),
    partial: download?.entries.next_cursor != null,
  };
}

export function usePlaylistDownloadAction() {
  const { checking, pending, requested, retryKind, start, remove, retry } =
    useDownloadState();
  const retrying = retryKind !== null && !pending;
  const label = retrying
    ? retryKind === "remove"
      ? "Retry removal"
      : "Retry download request"
    : requested
      ? "Remove download"
      : "Download";
  const action = retryKind ? retry : requested ? remove : start;
  return {
    label,
    icon: !requested && !retrying,
    disabled: checking || pending,
    run: () => void action(),
  };
}

export function PlaylistDownloadButton() {
  const download = usePlaylistDownloadAction();
  return (
    <Button
      className="tint-secondary"
      disabled={download.disabled}
      onClick={download.run}
    >
      {download.icon && <Download size={17} aria-hidden="true" />}
      {download.label}
    </Button>
  );
}

export function PlaylistDownloadMenu({
  label,
  items,
}: {
  label: string;
  items: ActionMenuItem[];
}) {
  const download = usePlaylistDownloadAction();
  return (
    <ActionMenu
      label={label}
      triggerClassName="icon-round"
      items={[
        ...items.slice(0, 1),
        {
          label: download.label,
          disabled: download.disabled,
          onSelect: download.run,
        },
        ...items.slice(1),
      ]}
    />
  );
}

export function PlaylistDownloadStrip({ local }: { local: boolean }) {
  const {
    download,
    checking,
    pending,
    requested,
    hasRetainedFailures,
    retryKind,
    error,
    statusError,
    retryFailed,
    check,
  } = useDownloadState();
  const { entries, ready, waiting, lowStorage, failed, unavailable, partial } =
    summary(download);
  const visible = download || checking || requested || error || statusError;
  if (!visible) return null;
  return (
    <section
      className="status-strip download-strip"
      aria-labelledby="playlist-download-heading"
    >
      <h2 id="playlist-download-heading" className="sr-only">
        Playlist download
      </h2>
      <div className="status-strip-main">
        <p className="status-strip-note">
          Copies are saved on this {local ? "device" : "server"}.
          {!local && (
            <>
              {" "}
              <span>
                This browser still needs a connection to this server to listen.
              </span>
            </>
          )}
        </p>
        <p role="status" className="status-strip-line">
          {download
            ? entries.length === 0
              ? "No playlist entries to download."
              : `${ready} of ${entries.length} ${partial ? "checked entries" : "entries"} saved.`
            : checking
              ? "Checking saved copies…"
              : requested
                ? "Download requested."
                : null}
        </p>
        {download && entries.length > 0 && (
          <progress
            aria-label="Playlist download progress"
            max={entries.length}
            value={ready}
          />
        )}
        {partial && (
          <p className="status-strip-note">
            Only the first {entries.length} entries are checked here. More
            entries have not been checked.
          </p>
        )}
        {download &&
          download.reconciled_revision < download.desired_revision && (
            <p className="status-strip-note">
              Updating playlist download membership…
            </p>
          )}
        {waiting > 0 && (
          <p className="status-strip-note">
            {waiting} {waiting === 1 ? "entry" : "entries"} waiting for a
            source.
          </p>
        )}
        {lowStorage > 0 && (
          <p className="status-strip-note">
            {lowStorage} {lowStorage === 1 ? "entry is" : "entries are"} waiting
            for disk space on this {local ? "device" : "server"}. Hito will try
            again.
          </p>
        )}
        {failed > 0 && (
          <p className="status-strip-note">
            {failed} {failed === 1 ? "entry could" : "entries could"} not be
            downloaded.
          </p>
        )}
        {unavailable > 0 && (
          <p className="status-strip-note">
            {unavailable} {unavailable === 1 ? "entry is" : "entries are"}{" "}
            unavailable.
          </p>
        )}
        {error && (
          <p className="form-error" role="alert">
            {error}
          </p>
        )}
        {statusError && (
          <p className="form-error" role="alert">
            {statusError}
          </p>
        )}
      </div>
      {(statusError || (hasRetainedFailures && failed > 0)) && (
        <div className="status-strip-actions">
          {statusError && (
            <Button
              disabled={checking || pending || retryKind !== null}
              onClick={() => void check()}
            >
              Check download
            </Button>
          )}
          {hasRetainedFailures && failed > 0 && (
            <Button
              disabled={checking || pending || retryKind !== null}
              onClick={() => void retryFailed()}
            >
              Retry failed downloads
            </Button>
          )}
        </div>
      )}
    </section>
  );
}
