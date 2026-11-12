import type { Schema } from "../../../shared/schema";
import AlbumGrid from "../../catalog/components/AlbumGrid";

export default function OwnAlbums({
  releases,
}: {
  releases: Schema<"Release">[];
}) {
  if (releases.length === 0) return null;
  return (
    <section className="detail-section" aria-labelledby="own-albums-title">
      <h2 id="own-albums-title">Albums in your library</h2>
      <AlbumGrid releases={releases} />
    </section>
  );
}
