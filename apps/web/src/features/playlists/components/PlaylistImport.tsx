import type { Schema } from "../../../shared/schema";
import { useRef, useState, type FormEvent } from "react";
import { Link } from "@inertiajs/react";
import { ChevronDown, Link2 } from "lucide-react";
import {
  useProviderPlaylist,
  type PlaylistCreationMode,
} from "../hooks/useProviderPlaylist";
import { importProblem } from "../model/importProblem";
import { inferPlaylistService } from "../model/playlistService";
import Button from "../../../shared/ui/Button";
import ImportProgress from "./ImportProgress";

type Interval = Schema<"PlaylistFollowInterval">;

const MODES: { value: PlaylistCreationMode; label: string; hint: string }[] = [
  {
    value: "follow",
    label: "Follow",
    hint: "Stays the same as on the service. You can't edit it.",
  },
  {
    value: "copy",
    label: "Copy",
    hint: "Yours to edit. It won't change when the service does.",
  },
];

export default function PlaylistImport({
  account,
  connections,
  names,
  initialReference = "",
  onCancel,
}: {
  account: string;
  connections: Schema<"PluginInstallation">[];
  names: Record<string, string>;
  initialReference?: string;
  onCancel: () => void;
}) {
  const providers = connections.filter((connection) =>
    connection.manifest.capabilities.includes("playlists"),
  );
  const input = useRef<HTMLInputElement>(null);
  const [choice, setChoice] = useState("");
  const [reference, setReference] = useState(initialReference);
  const [chosenMode, setMode] = useState<PlaylistCreationMode | null>(null);
  const [interval, setInterval] = useState<Interval>("daily");
  const inferred = inferPlaylistService(reference, providers, names);
  const chosen = providers.find((provider) => provider.id === choice) ?? null;
  const selected = chosen ?? inferred.provider ?? providers[0] ?? null;
  const mode: PlaylistCreationMode =
    chosenMode ?? (inferred.recognised ? "follow" : "copy");
  const workflow = useProviderPlaylist(account, selected);
  const previousAttempt = workflow.attemptKind === mode;
  const serviceName = selected
    ? (names[selected.service_id] ?? selected.manifest.name)
    : "";
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
  if (!selected)
    return (
      <section className="playlist-import" aria-label="Import a playlist">
        <p className="muted">
          {connections.length
            ? "Your connected services do not offer playlist imports. Connect another service in Plugins."
            : "Connect a playlist service to copy a playlist into Hito."}
        </p>
        <div className="import-actions">
          <Link className="button action" href="/settings?section=plugins">
            Connect a playlist service
          </Link>
          <Button onClick={onCancel}>Cancel</Button>
        </div>
      </section>
    );
  if (workflow.pending)
    return (
      <section className="playlist-import" aria-label="Import a playlist">
        <ImportProgress
          kind={workflow.pendingKind ?? mode}
          service={serviceName}
        />
      </section>
    );
  const problem = workflow.error
    ? importProblem(
        workflow.errorStatus,
        serviceName,
        workflow.error,
        workflow.attemptKind !== "copy" && mode === "follow",
      )
    : null;
  return (
    <section className="playlist-import" aria-label="Import a playlist">
      <form className="playlist-import-form" onSubmit={submit}>
        {providers.length > 1 && (chosen || inferred.ambiguous) && (
          <label className="import-field">
            <span>Playlist service</span>
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
        )}
        <label className="import-field">
          <span>Playlist link or identifier</span>
          <span className="import-link">
            <Link2 size={18} aria-hidden="true" />
            <input
              ref={input}
              value={reference}
              onChange={(event) => setReference(event.target.value)}
              maxLength={2048}
              required
              autoComplete="off"
              spellCheck={false}
              placeholder="Paste a playlist link"
            />
          </span>
        </label>
        {!reference.trim() && (
          <p className="import-help">
            A link looks like open.spotify.com/playlist/… or
            youtube.com/playlist?list=…
          </p>
        )}
        {reference.trim() && (
          <div className="import-preview">
            <span className="import-preview-cover" aria-hidden="true" />
            <span>
              <strong>{serviceName}</strong>
              <small>
                {inferred.recognised
                  ? "Playlist link recognised"
                  : "Hito reads the playlist when you confirm"}
              </small>
            </span>
          </div>
        )}
        {problem && (
          <div className="import-problem" role="alert">
            <strong>{problem.title}</strong>
            <p>{problem.text}</p>
            <div className="import-problem-actions">
              {problem.actions.includes("edit") && (
                <button type="button" onClick={() => input.current?.focus()}>
                  Edit the link
                </button>
              )}
              {problem.actions.includes("copy") && (
                <button type="button" onClick={() => setMode("copy")}>
                  Import a copy
                </button>
              )}
              {problem.actions.includes("plugins") && (
                <Link href="/settings?section=plugins">Open Plugins</Link>
              )}
            </div>
          </div>
        )}
        <fieldset className="import-modes">
          <legend>How to add it</legend>
          {MODES.map((option) => (
            <label
              key={option.value}
              className="import-mode"
              data-checked={mode === option.value}
            >
              <input
                type="radio"
                name="import-mode"
                checked={mode === option.value}
                onChange={() => setMode(option.value)}
                onClick={() => setMode(option.value)}
              />
              <span>
                <strong>{option.label}</strong>
                <span>{option.hint}</span>
              </span>
            </label>
          ))}
        </fieldset>
        {mode === "follow" && (
          <div className="import-interval">
            <span>While you follow it</span>
            <label>
              <span>Check for changes</span>
              <select
                aria-label="Refresh interval"
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
              <ChevronDown size={14} aria-hidden="true" />
            </label>
          </div>
        )}
        {workflow.hasAttempt && (
          <div className="playlist-import-recovery">
            <p>
              {workflow.attemptKind === "copy"
                ? "Retry recovers the previous import. Import a new copy uses the current playlist field."
                : "Retry recovers the previous follow request and its original refresh interval. Following again uses the current playlist field and interval."}
            </p>
            <Button type="button" onClick={() => void workflow.retry()}>
              {workflow.attemptKind === "copy"
                ? "Retry import"
                : "Retry follow"}
            </Button>
          </div>
        )}
        <div className="import-actions">
          <Button variant="action" type="submit" disabled={!reference.trim()}>
            {submitLabel}
          </Button>
          <Button onClick={onCancel}>Cancel</Button>
        </div>
      </form>
    </section>
  );
}
