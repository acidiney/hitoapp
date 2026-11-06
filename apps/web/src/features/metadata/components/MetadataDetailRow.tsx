import type { Schema } from "../../../shared/schema";
import { useState, type FormEvent, type KeyboardEvent } from "react";
import { Pencil } from "lucide-react";
import type { MutationResult } from "../../../shared/hooks/useMutation";
import Button from "../../../shared/ui/Button";
import type { MetadataField } from "../model/metadata";

export default function MetadataDetailRow({
  field,
  metadata,
  pending,
  save: accept,
}: {
  field: MetadataField;
  metadata: Schema<"TextMetadata">;
  pending: boolean;
  save: (personal: string | null) => Promise<MutationResult>;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(metadata.value);
  const [error, setError] = useState("");
  const label = `Track ${field}`;
  const visible = field === "title" ? "Title" : "Artist";
  const inputId = `personal-recording-${field}`;
  const penId = `edit-recording-${field}`;
  const personal = metadata.personal !== null;

  function close() {
    setEditing(false);
    setError("");
    setDraft(metadata.value);
    requestAnimationFrame(() => document.getElementById(penId)?.focus());
  }

  async function save(inherit: boolean) {
    setError("");
    const result = await accept(inherit ? null : draft);
    if (!result.ok) setError(result.error);
    else close();
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    if (!pending) void save(false);
  }

  function press(event: KeyboardEvent) {
    if (event.key === "Escape" && !pending) {
      event.preventDefault();
      event.stopPropagation();
      close();
    }
  }

  const chip = (
    <span className="track-chip" data-tone={personal ? "action" : "neutral"}>
      {personal ? "Personal" : "Following source"}
    </span>
  );

  if (editing)
    return (
      <form
        className="track-row track-row-editing"
        onSubmit={submit}
        onKeyDown={press}
      >
        <label className="track-row-label" htmlFor={inputId}>
          {visible}
        </label>
        <div className="track-row-value">
          <input
            id={inputId}
            aria-label={label}
            value={draft}
            required
            autoFocus
            disabled={pending}
            aria-describedby={`${inputId}-inheritance`}
            onChange={(event) => setDraft(event.target.value)}
          />
          <span className="track-muted" id={`${inputId}-inheritance`}>
            {personal ? "Personal" : "Following source"} · source says{" "}
            <strong>{metadata.inherited ?? "Not provided by source"}</strong>.
            Applies to this account across albums and playlists.
          </span>
          {error && (
            <p className="form-error" role="alert">
              {error}
            </p>
          )}
          <div className="track-row-actions">
            {personal && (
              <Button disabled={pending} onClick={() => void save(true)}>
                Use source {field}
              </Button>
            )}
            <Button disabled={pending} onClick={close}>
              Cancel
            </Button>
            <Button
              variant="primary"
              type="submit"
              disabled={
                pending ||
                !draft.trim() ||
                (personal && draft === metadata.personal)
              }
            >
              {pending ? "Saving…" : `Save personal ${field}`}
            </Button>
          </div>
        </div>
      </form>
    );

  return (
    <div className="track-row">
      <span className="track-row-label">{visible}</span>
      <div className="track-row-value">
        <span>{metadata.value}</span>
        {chip}
        {personal && metadata.inherited && (
          <span className="track-muted">
            Source says “{metadata.inherited}”
          </span>
        )}
      </div>
      <button
        id={penId}
        type="button"
        className="button secondary track-row-edit"
        aria-label={`Edit ${label.toLowerCase()}`}
        onClick={() => setEditing(true)}
      >
        <Pencil size={16} aria-hidden="true" />
      </button>
    </div>
  );
}
