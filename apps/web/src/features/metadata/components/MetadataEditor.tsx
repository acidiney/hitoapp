import type { MetadataEditorInput } from "../model/metadata";
import { useMetadataEditor } from "../hooks/useMetadataEditor";
import MetadataFieldEditor from "./MetadataFieldEditor";

export default function MetadataEditor(
  props: { account: string } & MetadataEditorInput,
) {
  const { save, pending } = useMetadataEditor(props.account, props);
  const recording = props.kind === "recording";
  return (
    <section
      className="rd-card metadata-editor"
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
          key={`${field}/${props.metadata[field].operation_id ?? "inherited"}`}
          target={props}
          field={field}
          metadata={props.metadata[field]}
          pending={pending}
          save={(personal) => save(field, personal)}
        />
      ))}
    </section>
  );
}
