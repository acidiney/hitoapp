import { Folder, FolderSync } from "lucide-react";
import type { SettingsPageProps } from "../types";
import { useScanJob } from "../hooks/useScanJob";

export default function LibrarySettings({
  user,
  libraries,
}: SettingsPageProps) {
  const { scan, scanning } = useScanJob(user.id, libraries[0]?.id);
  return (
    <section className="settings-panel" aria-labelledby="storage-title">
      <div className="settings-intro">
        <h2 id="storage-title">Your music, in one place</h2>
        <p>
          Hito reads your files and their tags. Your originals stay exactly
          where they are.
        </p>
      </div>
      <div className="setting-row">
        <div>
          <h3>Connected libraries</h3>
          <p>Libraries this account can listen to.</p>
        </div>
        <span className="setting-value">{libraries.length}</span>
      </div>
      {libraries.map((library) => (
        <div className="library-storage-row" key={library.id}>
          <span className="storage-icon">
            <Folder size={22} />
          </span>
          <span>
            <strong>{library.name}</strong>
            <small>Original files · Read-only</small>
          </span>
        </div>
      ))}
      {user.admin && libraries.length > 0 && (
        <div className="setting-row">
          <div>
            <h3>Update your library</h3>
            <p>Find new files and refresh their tags.</p>
          </div>
          <button
            className="button"
            disabled={scanning}
            onClick={() => void scan()}
          >
            <FolderSync size={17} />
            {scanning ? "Scanning music…" : "Scan music folder"}
          </button>
        </div>
      )}
      {user.admin && (
        <details className="storage-help">
          <summary>Change the music folder</summary>
          <p>
            Set <code>HITO_MUSIC_FOLDER</code> in the Docker environment and
            recreate the server. Music folders are mounted read-only.
          </p>
        </details>
      )}
    </section>
  );
}
