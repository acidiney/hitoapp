import { useEffect, useRef, useState } from "react";
import {
  Folder,
  FolderOpen,
  FolderPlus,
  FolderSync,
  Trash2,
} from "lucide-react";
import type { Library, MusicFolder } from "../../../shared/types";
import { useHostFolders } from "../hooks/useHostFolders";
import { useMusicFolders } from "../hooks/useMusicFolders";
import FolderPicker from "./FolderPicker";

function status(folder: MusicFolder) {
  const job = folder.latest_job;
  if (!folder.available)
    return "Unavailable. Check that the drive is connected.";
  if (job?.state === "queued") return "Waiting to scan";
  if (job?.state === "running")
    return `Scanning · ${job.progress.toLocaleString()} files read`;
  if (job?.state === "failed")
    return "Scan failed. Check the folder and try again.";
  if (job?.state === "complete")
    return `${job.progress.toLocaleString()} ${job.progress === 1 ? "file" : "files"} read`;
  return "Ready to scan";
}

export default function MusicFolders({
  account,
  library,
}: {
  account: string;
  library: Library;
}) {
  const folders = useMusicFolders(account, library.id);
  const host = useHostFolders(
    account,
    folders.folders,
    !folders.loading,
    folders.connect,
  );
  const [picking, pick] = useState(false);
  const [removing, remove] = useState<string | null>(null);
  const addButton = useRef<HTMLButtonElement>(null);
  const wasPicking = useRef(false);
  useEffect(() => {
    if (wasPicking.current && !picking)
      addButton.current?.focus({ preventScroll: true });
    wasPicking.current = picking;
  }, [picking]);
  return (
    <>
      <div className="music-folders-heading">
        <div>
          <h3>Music folders</h3>
          <p>{library.name} · Read-only access</p>
        </div>
        <div className="music-folders-add">
          {host.picker && (
            <button
              className="button"
              disabled={!!folders.pending || host.choosing}
              onClick={() => void host.choose()}
            >
              <FolderOpen size={18} />
              {host.choosing ? "Waiting for your choice…" : "Choose folder…"}
            </button>
          )}
          <button
            ref={addButton}
            className="button primary"
            disabled={!!folders.pending}
            onClick={() => pick(true)}
          >
            <FolderPlus size={18} />
            Add music folder
          </button>
        </div>
      </div>
      {host.suggestion && (
        <div className="music-folder-proposal">
          <h3>Add your Music folder?</h3>
          <p>
            Hito found the Music folder on this computer. It only reads your
            files and never changes them.
          </p>
          <div>
            <button className="button" onClick={host.dismiss}>
              Not now
            </button>
            <button
              className="button primary"
              disabled={!!folders.pending || host.choosing}
              onClick={() => void host.accept()}
            >
              Use Music folder
            </button>
          </div>
        </div>
      )}
      {host.error && (
        <p className="form-error" role="alert">
          {host.error}
        </p>
      )}
      {folders.loading && <p role="status">Loading music folders…</p>}
      {!folders.loading && !folders.folders.length && (
        <div className="music-folders-empty">
          <Folder size={32} aria-hidden="true" />
          <h3>Make room for your collection</h3>
          <p>
            Add a folder to find your music. Hito reads the audio and its tags
            without moving your files.
          </p>
        </div>
      )}
      <ul className="music-folders-list" aria-label="Connected music folders">
        {folders.folders.map((folder) => (
          <li key={folder.id}>
            <div className="music-folder-row">
              <span className="storage-icon">
                <Folder size={22} />
              </span>
              <div className="music-folder-detail">
                <h4>{folder.name}</h4>
                <p role="status">{status(folder)}</p>
                {!!folder.directory?.components?.length && (
                  <small>{folder.directory.components.join(" / ")}</small>
                )}
              </div>
              <div className="music-folder-actions">
                <button
                  className="icon-button"
                  aria-label={`Scan ${folder.name}`}
                  disabled={
                    !!folders.pending ||
                    !folder.available ||
                    folder.latest_job?.state === "queued" ||
                    folder.latest_job?.state === "running"
                  }
                  onClick={() => void folders.scan(folder)}
                >
                  <FolderSync size={19} />
                </button>
                <button
                  className="icon-button"
                  aria-label={`Remove ${folder.name}`}
                  disabled={!!folders.pending}
                  onClick={() => remove(folder.id)}
                >
                  <Trash2 size={18} />
                </button>
              </div>
            </div>
            {removing === folder.id && (
              <div className="folder-remove-confirm">
                <h4>Remove {folder.name}?</h4>
                <p>
                  Your files stay on disk. Playlists, likes, and library records
                  are kept. Music available only in this folder will become
                  unavailable.
                </p>
                <div>
                  <button
                    className="button"
                    disabled={!!folders.pending}
                    onClick={() => remove(null)}
                  >
                    Keep folder
                  </button>
                  <button
                    className="button danger"
                    disabled={!!folders.pending}
                    onClick={async () => {
                      if (await folders.remove(folder)) remove(null);
                    }}
                  >
                    {folders.pending === folder.id
                      ? "Removing…"
                      : "Remove folder"}
                  </button>
                </div>
              </div>
            )}
          </li>
        ))}
      </ul>
      {folders.error && !picking && (
        <div role="alert" className="folder-error">
          <p className="form-error">{folders.error}</p>
          <button className="text-link" onClick={folders.retry}>
            Refresh folders
          </button>
        </div>
      )}
      {!!folders.folders.length && (
        <div className="setting-row">
          <div>
            <h3>Refresh your collection</h3>
            <p>
              Find new files and read changed tags in all connected folders.
            </p>
          </div>
          <button
            className="button"
            disabled={!!folders.pending}
            onClick={() => void folders.scanAll()}
          >
            <FolderSync size={17} />
            Scan all folders
          </button>
        </div>
      )}
      {!host.picker && (
        <p className="folder-host-help">
          Looking for a folder on another drive? Open Hito Setup on the server
          computer to share it, then choose it here.
        </p>
      )}
      {picking && (
        <FolderPicker
          account={account}
          pending={!!folders.pending}
          error={folders.error}
          onSelect={folders.connect}
          onClose={() => pick(false)}
        />
      )}
    </>
  );
}
