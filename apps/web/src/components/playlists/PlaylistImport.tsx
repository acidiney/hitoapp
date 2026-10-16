import { useState, type FormEvent } from "react";
import { Link } from "@inertiajs/react";
import { usePlaylistImport } from "../../hooks/usePlaylistImport";
import type { PluginInstallation } from "../../types";

export default function PlaylistImport({
  account,
  connections,
  names,
}: {
  account: string;
  connections: PluginInstallation[];
  names: Record<string, string>;
}) {
  const providers = connections.filter((connection) =>
    connection.manifest.capabilities.includes("playlists"),
  );
  const [choice, setChoice] = useState("");
  const [reference, setReference] = useState("");
  const selected =
    providers.find((provider) => provider.id === choice) ??
    providers[0] ??
    null;
  const workflow = usePlaylistImport(account, selected);
  function submit(event: FormEvent) {
    event.preventDefault();
    void workflow.importCopy(reference);
  }

  return (
    <section className="playlist-import" aria-label="Import a playlist">
      <h2>Import a playlist</h2>
      <p className="muted">
        Create an editable copy. Changes at the provider will not update it.
        Copying a playlist adds its track details, without downloading audio.
        Tracks need an available source to play.
      </p>
      {selected ? (
        <form className="playlist-import-form" onSubmit={submit}>
          <label>
            Playlist service
            <select
              value={selected.id}
              onChange={(event) => setChoice(event.target.value)}
            >
              {providers.map((provider) => (
                <option key={provider.id} value={provider.id}>
                  {names[provider.service_id] ?? provider.manifest.name}
                </option>
              ))}
            </select>
          </label>
          <label>
            Playlist link or identifier
            <input
              value={reference}
              onChange={(event) => setReference(event.target.value)}
              maxLength={2048}
              required
              autoComplete="off"
              spellCheck={false}
              placeholder="Paste a playlist link or identifier"
            />
          </label>
          <button
            className="button primary"
            type="submit"
            disabled={workflow.pending || !reference.trim()}
          >
            {workflow.pending
              ? "Importing…"
              : workflow.hasAttempt
                ? "Import a new copy"
                : "Import a copy"}
          </button>
          {workflow.error && <p role="alert">{workflow.error}</p>}
          {workflow.hasAttempt && !workflow.pending && (
            <div className="playlist-import-recovery">
              <p className="muted">
                Retry recovers the previous import. Import a new copy uses the
                current playlist field.
              </p>
              <button
                className="button"
                type="button"
                disabled={workflow.pending}
                onClick={() => void workflow.retry()}
              >
                Retry import
              </button>
            </div>
          )}
        </form>
      ) : (
        <div>
          <p className="muted">
            {connections.length
              ? "Your connected services do not offer playlist imports. Connect another service in Settings."
              : "Connect a playlist service to copy a playlist into Hito."}
          </p>
          <Link className="button" href="/settings?section=plugins">
            Connect a playlist service
          </Link>
        </div>
      )}
    </section>
  );
}
