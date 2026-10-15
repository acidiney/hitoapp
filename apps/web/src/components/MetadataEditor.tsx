import { router } from "@inertiajs/react";
import type { RecordingMetadata } from "../types";
import {
  metadataAction,
  type MetadataTarget,
  type MetadataField,
} from "../metadata";
import { useMutation } from "../hooks/useMutation";
import { usePlayer } from "../player/PlaybackProvider";
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
  const { mutate, pending } = useMutation(account);
  const player = usePlayer();
  const recording = target.kind === "recording";
  async function save(field: MetadataField, personal: string | null) {
    const result = await mutate(metadataAction(target, field, personal));
    if (result.ok) {
      if (recording)
        player.setRecordingField(
          target.id,
          field,
          personal,
          metadata[field].inherited,
        );
      router.reload();
    }
    return result;
  }
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
