import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { useMetadataEditor } from "../hooks/useMetadataEditor";
import type { MetadataField } from "../model/metadata";
import Button from "../../../shared/ui/Button";
import MetadataDetailRow from "./MetadataDetailRow";

const fields = ["title", "artist"] as const satisfies MetadataField[];

export default function MetadataDetails({
  account,
  detail,
}: {
  account: string;
  detail: Schema<"RecordingMetadata">;
}) {
  const target = {
    kind: "recording",
    id: detail.recording_id,
    metadata: detail,
  } as const;
  const { save, pending } = useMetadataEditor(account, target);
  const [error, setError] = useState("");
  const edited = fields.filter((field) => detail[field].personal !== null);
  async function reset() {
    setError("");
    for (const field of edited) {
      const result = await save(field, null);
      if (!result.ok) {
        setError(result.error);
        return;
      }
    }
  }
  return (
    <section className="track-block" aria-labelledby="personal-details-heading">
      <div className="track-block-head">
        <h2 id="personal-details-heading">Details</h2>
        {edited.length > 0 && (
          <div className="track-toolbar">
            <span className="track-chip" data-tone="action">
              {edited.length} personal {edited.length === 1 ? "edit" : "edits"}
            </span>
            <Button
              variant="secondary"
              disabled={pending}
              onClick={() => void reset()}
            >
              Reset all to source
            </Button>
          </div>
        )}
      </div>

      {error && (
        <p className="form-error" role="alert">
          {error}
        </p>
      )}
      <div className="track-rows">
        {fields.map((field) => (
          <MetadataDetailRow
            key={`${field}/${detail[field].operation_id ?? "inherited"}`}
            field={field}
            metadata={detail[field]}
            pending={pending}
            save={(personal) => save(field, personal)}
          />
        ))}
      </div>
    </section>
  );
}
