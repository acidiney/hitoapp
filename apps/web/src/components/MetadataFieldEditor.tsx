import { useState, type FormEvent } from "react";
import { router } from "@inertiajs/react";
import type { TextMetadata } from "../types";
import {
  metadataAction,
  type MetadataTarget,
  type MetadataField,
} from "../metadata";
import { useMutation } from "../hooks/useMutation";
import { usePlayer } from "../player/PlaybackProvider";

export default function MetadataFieldEditor({
  account,
  target,
  field,
  metadata,
}: {
  account: string;
  target: MetadataTarget;
  field: MetadataField;
  metadata: TextMetadata;
}) {
  const [draft, setDraft] = useState(metadata.value);
  const [error, setError] = useState("");
  const { mutate, pending } = useMutation(account);
  const player = usePlayer();
  const label = `${target.kind === "recording" ? "Track" : "Album"} ${field}`;
  const inputId = `personal-${target.kind}-${field}`;
  async function save(inherit: boolean) {
    setError("");
    const personal = inherit ? null : draft;
    const result = await mutate(metadataAction(target, field, personal));
    if (!result.ok) {
      setError(result.error);
      return;
    }
    if (target.kind === "recording")
      player.setRecordingField(target.id, field, personal, metadata.inherited);
    router.reload();
  }
  function submit(event: FormEvent) {
    event.preventDefault();
    if (!pending) void save(false);
  }
  return (
    <form className="metadata-field" onSubmit={submit}>
      <label htmlFor={inputId}>{label}</label>
      <input
        id={inputId}
        value={draft}
        required
        disabled={pending}
        aria-describedby={`${inputId}-inheritance`}
        onChange={(event) => setDraft(event.target.value)}
      />
      <span className="metadata-inheritance" id={`${inputId}-inheritance`}>
        {metadata.personal === null
          ? "Following source metadata"
          : `Personal ${field}`}{" "}
        · Source {field}: <strong>{metadata.inherited}</strong>
      </span>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      <div className="metadata-editor-actions">
        {metadata.personal !== null && (
          <button
            type="button"
            className="button"
            disabled={pending}
            onClick={() => void save(true)}
          >
            Use source {field}
          </button>
        )}
        <button
          className="button primary"
          disabled={
            pending ||
            !draft.trim() ||
            (metadata.personal !== null && draft === metadata.personal)
          }
        >
          {pending ? "Saving…" : `Save personal ${field}`}
        </button>
      </div>
    </form>
  );
}
