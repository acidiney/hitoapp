import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";

import Artwork from "../../artwork/components/Artwork";

export default function AlbumGrid({
  releases,
}: {
  releases: Schema<"Release">[];
}) {
  return (
    <div className="album-grid">
      {releases.map((release) => (
        <article className="album-card" key={release.id}>
          <Link
            href={`/albums/${encodeURIComponent(release.id)}`}
            className="album-cover-link"
            aria-label={
              release.artist !== null && release.artist_id !== null
                ? `Open ${release.title} by ${release.artist}`
                : `Open ${release.title}`
            }
          >
            <Artwork
              release={release.id}
              fallback={{
                id: release.id,
                title: release.title,
                artist: release.artist,
              }}
            />
          </Link>
          <h2>
            <Link href={`/albums/${encodeURIComponent(release.id)}`}>
              {release.title}
            </Link>
          </h2>
          {release.artist !== null && release.artist_id !== null ? (
            <Link
              className="credit-link"
              href={`/artists/${encodeURIComponent(release.artist_id)}`}
            >
              {release.artist}
            </Link>
          ) : null}
          <small>
            {release.track_count}{" "}
            {release.track_count === 1 ? "track" : "tracks"}
          </small>
        </article>
      ))}
    </div>
  );
}
