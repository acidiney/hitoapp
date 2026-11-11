import type { Schema } from "../../../shared/schema";
import Artwork from "../../artwork/components/Artwork";
import PlaylistMosaic from "../../artwork/components/PlaylistMosaic";

export default function GenreCover({
  albums,
  title,
}: {
  albums: Schema<"Release">[];
  title: string;
}) {
  if (albums.length >= 4)
    return (
      <PlaylistMosaic
        covers={albums.slice(0, 4).map((album) => ({
          key: album.id,
          release: album.id,
          recording: album.artwork_recording_id ?? album.id,
          title: album.title,
        }))}
      />
    );
  const first = albums[0];
  return (
    <Artwork
      release={first?.id}
      recording={first?.artwork_recording_id ?? undefined}
      fallback={{ id: first?.id ?? title, title, kind: "album" }}
    />
  );
}
