import { useState, type FormEvent } from "react";
import { Link } from "@inertiajs/react";
import {
  useProviderPlaylist,
  type PlaylistCreationMode,
} from "../hooks/useProviderPlaylist";
import type {
  PlaylistFollowInterval,
  PluginInstallation,
} from "../../../shared/types";

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
  const [mode, setMode] = useState<PlaylistCreationMode>("copy");
  const [interval, setInterval] = useState<PlaylistFollowInterval>("hourly");
  const selected =
    providers.find((provider) => provider.id === choice) ??
    providers[0] ??
    null;
  const workflow = useProviderPlaylist(account, selected);
  const previousAttempt = workflow.attemptKind === mode;
  const submitLabel =
    mode === "copy"
      ? previousAttempt
        ? "Import a new copy"
        : "Import a copy"
      : previousAttempt
        ? "Follow as a new playlist"
        : "Follow playlist";
  function submit(event: FormEvent) {
    event.preventDefault();
    void workflow.create(reference, mode, interval);
  }

  return (
    <section className="playlist-import" aria-label="Import a playlist">
      <h2>Import a playlist</h2>
      <p className="muted">
        {mode === "copy"
          ? "Create an editable copy. Changes at the provider will not update it. Copying a playlist adds its track details, without downloading audio. Tracks need an available source to play."
          : "Follow a read-only playlist. Hito checks its provider for changes to tracks and their order. Following adds track details, without downloading audio. Tracks need an available source to play."}
      </p>
      {selected ? (
        <form className="playlist-import-form" onSubmit={submit}>
          <fieldset>
            <legend>Playlist behavior</legend>
            <label>
              <input
                type="radio"
                name="playlist-behavior"
                checked={mode === "copy"}
                onChange={() => setMode("copy")}
              />
              Import a copy
            </label>
            <label>
              <input
                type="radio"
                name="playlist-behavior"
                checked={mode === "follow"}
                onChange={() => setMode("follow")}
              />
              Follow playlist
            </label>
          </fieldset>
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
          {mode === "follow" && (
            <label>
              Refresh interval
              <select
                value={interval}
                onChange={(event) => {
                  const value = event.target.value;
                  if (value === "hourly" || value === "daily")
                    setInterval(value);
                }}
              >
                <option value="hourly">Every hour</option>
                <option value="daily">Every day</option>
              </select>
            </label>
          )}
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
              ? workflow.pendingKind === "copy"
                ? "Importing…"
                : "Following…"
              : submitLabel}
          </button>
          {workflow.error && <p role="alert">{workflow.error}</p>}
          {workflow.hasAttempt && !workflow.pending && (
            <div className="playlist-import-recovery">
              <p className="muted">
                {workflow.attemptKind === "copy"
                  ? "Retry recovers the previous import. Import a new copy uses the current playlist field."
                  : "Retry recovers the previous follow request and its original refresh interval. Following again uses the current playlist field and interval."}
              </p>
              <button
                className="button"
                type="button"
                disabled={workflow.pending}
                onClick={() => void workflow.retry()}
              >
                {workflow.attemptKind === "copy"
                  ? "Retry import"
                  : "Retry follow"}
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
