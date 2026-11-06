import type { Schema } from "../../../shared/schema";
import { router } from "@inertiajs/react";
import { X } from "lucide-react";
import type { Mutate } from "../../../shared/hooks/useMutation";

export default function PlaylistEntryRemove({
  entryId,
  title,
  playlist,
  mutate,
}: {
  entryId: string;
  title: string;
  playlist: Schema<"Playlist">;
  mutate: Mutate;
}) {
  if (playlist.kind !== "local") return null;
  async function remove() {
    const result = await mutate({
      kind: "entry_remove",
      playlist_id: playlist.id,
      entry_id: entryId,
    });
    if (result.ok) router.reload();
  }
  return (
    <button
      className="icon-button"
      aria-label={`Remove ${title} from playlist`}
      onClick={() => void remove()}
    >
      <X size={16} />
    </button>
  );
}
