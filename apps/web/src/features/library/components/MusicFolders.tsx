import type { Schema } from "../../../shared/schema";
import { useEffect, useRef, useState } from "react";
import { Folder } from "lucide-react";

import Button from "../../../shared/ui/Button";
import SectionWarning from "../../../shared/ui/SectionWarning";
import SettingRow from "../../../shared/ui/SettingRow";
import SettingsCard from "../../../shared/ui/SettingsCard";
import { useHostFolders } from "../hooks/useHostFolders";
import { useMusicFolders } from "../hooks/useMusicFolders";
import FolderPicker from "./FolderPicker";
import FolderRow from "./FolderRow";
import MissingRecordings from "./MissingRecordings";
import { setting } from "../../settings/model/settingsCatalog";

export default function MusicFolders({
  account,
  library,
}: {
  account: string;
  library: Schema<"Library">;
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
  const busy = !!folders.pending;
  useEffect(() => {
    if (wasPicking.current && !picking)
      addButton.current?.focus({ preventScroll: true });
    wasPicking.current = picking;
  }, [picking]);
  return (
    <SettingsCard
      setting={setting("music-folders")}
      description="Your originals stay exactly where they are."
      action={
        <div className="card-actions">
          {host.picker && (
            <Button
              disabled={busy || host.choosing}
              onClick={() => void host.choose()}
            >
              {host.choosing ? "Waiting for your choice…" : "Choose folder…"}
            </Button>
          )}
          <Button
            ref={addButton}
            disabled={busy}
            aria-label="Add music folder"
            onClick={() => pick(true)}
          >
            Add folder
          </Button>
        </div>
      }
    >
      {host.suggestion && (
        <div className="music-folder-proposal">
          <h4>Add your Music folder?</h4>
          <p>
            Hito found the Music folder on this computer. It only reads your
            files and never changes them.
          </p>
          <div>
            <Button onClick={host.dismiss}>Not now</Button>
            <Button
              variant="primary"
              disabled={busy || host.choosing}
              onClick={() => void host.accept()}
            >
              Use Music folder
            </Button>
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
          <h4>Make room for your collection</h4>
          <p>
            Add a folder to find your music. Hito reads the audio and its tags
            without moving your files.
          </p>
        </div>
      )}
      <ul className="plain-list" aria-label="Connected music folders">
        {folders.folders.map((folder) => (
          <li key={folder.id}>
            <FolderRow
              folder={folder}
              busy={busy}
              pending={folders.pending === folder.id}
              removing={removing === folder.id}
              error={removing === folder.id ? folders.error : ""}
              onScan={() => void folders.scan(folder)}
              onAskRemove={() => remove(folder.id)}
              onCancelRemove={() => remove(null)}
              onRemove={async () => {
                if (await folders.remove(folder)) remove(null);
              }}
            />
          </li>
        ))}
      </ul>
      {folders.error && !picking && removing === null && (
        <SectionWarning tone="error" title={folders.error}>
          <button className="text-link" onClick={folders.retry}>
            Refresh folders
          </button>
        </SectionWarning>
      )}
      {!!folders.folders.length && (
        <SettingRow
          title="Scan all folders"
          hint="Finds new files and changed tags."
          action={
            <Button
              disabled={busy}
              aria-label="Scan all folders"
              onClick={() => void folders.scanAll()}
            >
              Scan all
            </Button>
          }
        />
      )}
      <MissingRecordings
        account={account}
        library={library.id}
        disabled={busy}
      />
      {!host.picker && (
        <p className="folder-host-help muted">
          Folder on another drive? Share it in Hito Setup on the server
          computer, then add it here.
        </p>
      )}
      {picking && (
        <FolderPicker
          account={account}
          pending={busy}
          error={folders.error}
          onSelect={folders.connect}
          onClose={() => pick(false)}
        />
      )}
    </SettingsCard>
  );
}
