import { useState } from "react";
import { Folder } from "lucide-react";
import type { SettingsPageProps } from "../../../shared/types";
import MusicFolders from "./MusicFolders";

export default function LibrarySettings({
  user,
  libraries,
}: SettingsPageProps) {
  const shared = libraries.filter((library) => !library.personal);
  const [selected, select] = useState(shared[0]?.id);
  const library = shared.find((item) => item.id === selected) ?? shared[0];
  return (
    <section className="settings-panel" aria-labelledby="storage-title">
      <div className="settings-intro">
        <h2 id="storage-title">Your music, in one place</h2>
        <p>
          Bring your music folders together. Your originals stay exactly where
          they are.
        </p>
      </div>
      {user.admin && library ? (
        <>
          {shared.length > 1 && (
            <label className="folder-library-select">
              Library
              <select
                value={library.id}
                onChange={(event) => select(event.target.value)}
              >
                {shared.map((item) => (
                  <option key={item.id} value={item.id}>
                    {item.name}
                  </option>
                ))}
              </select>
            </label>
          )}
          <MusicFolders
            key={`${user.id}\0${library.id}`}
            account={user.id}
            library={library}
          />
        </>
      ) : (
        <>
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
                <small>
                  {library.personal
                    ? "Your connected plugins"
                    : "Managed by your administrator"}
                </small>
              </span>
            </div>
          ))}
          {!libraries.length && (
            <p>
              No libraries are connected to this account yet. Ask your
              administrator for access.
            </p>
          )}
        </>
      )}
      {user.admin && libraries.some((item) => item.personal) && (
        <p>Your personal provider library is managed in Plugins.</p>
      )}
    </section>
  );
}
