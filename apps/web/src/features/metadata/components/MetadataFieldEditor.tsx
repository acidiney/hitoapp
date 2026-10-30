import type { Schema } from "../../../shared/schema";
import { useState, type FormEvent } from "react";

import { type MetadataTarget, type MetadataField } from "../model/metadata";
import type { MutationResult } from "../../../shared/hooks/useMutation";
import Button from "../../../shared/ui/Button";

export default function MetadataFieldEditor({
  target,
  field,
  metadata,
  pending,
  save: accept,
}: {
  target: MetadataTarget;
  field: MetadataField;
  metadata: Schema<"TextMetadata"> | Schema<"ReleaseMetadata">["artist"];
  pending: boolean;
  save: (personal: string | null) => Promise<MutationResult>;
}) {
  const [draft, setDraft] = useState(metadata.value ?? "");
  const [error, setError] = useState("");
  const label = `${target.kind === "recording" ? "Track" : "Album"} ${field}`;
  const inputId = `personal-${target.kind}-${field}`;
  async function save(inherit: boolean) {
    setError("");
    const personal = inherit ? null : draft;
    const result = await accept(personal);
    if (!result.ok) {
      setError(result.error);
      return;
    }
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
        · Source {field}:{" "}
        <strong>{metadata.inherited ?? "Not provided by source"}</strong>
      </span>
      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      <div className="metadata-editor-actions">
        {metadata.personal !== null && (
          <Button
            type="button"
            disabled={pending}
            onClick={() => void save(true)}
          >
            Use source {field}
          </Button>
        )}
        <Button
          variant="primary"
          type="submit"
          disabled={
            pending ||
            !draft.trim() ||
            (metadata.personal !== null && draft === metadata.personal)
          }
        >
          {pending ? "Saving…" : `Save personal ${field}`}
        </Button>
      </div>
    </form>
  );
}
