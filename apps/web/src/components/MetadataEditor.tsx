import type { RecordingMetadata } from "../types";
import type { MetadataTarget } from "../metadata";
import { useMetadataEditor } from "../hooks/useMetadataEditor";
import MetadataFieldEditor from "./MetadataFieldEditor";

export default function MetadataEditor({
  account,
  target,
  metadata,
}: {
  account: string;
  target: MetadataTarget;
  metadata: Pick<RecordingMetadata, "title" | "artist">;
}) {
  const { save, pending } = useMetadataEditor(account, target, metadata);
  const recording = target.kind === "recording";
  return (
    <section
      className="metadata-editor"
      aria-labelledby="personal-details-heading"
    >
      <h2 id="personal-details-heading">
        Your {recording ? "track" : "album"} details
      </h2>
      <p>
        {recording
          ? "Changes apply to your account across this recording's albums and playlists."
          : "Changes apply to this album in your account. Track artists and source tags stay as recorded."}
      </p>
      {(["title", "artist"] as const).map((field) => (
        <MetadataFieldEditor
          key={`${field}/${metadata[field].operation_id ?? "inherited"}`}
          target={target}
          field={field}
          metadata={metadata[field]}
          pending={pending}
          save={(personal) => save(field, personal)}
        />
      ))}
    </section>
  );
}
