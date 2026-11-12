import { Link } from "@inertiajs/react";
import type { Schema } from "../../../shared/schema";
import Artwork from "../../artwork/components/Artwork";

export default function UnmatchedAlbums({
  releases,
}: {
  releases: Schema<"Release">[];
}) {
  if (releases.length === 0) return null;
  return (
    <section
      className="detail-section"
      aria-labelledby="unmatched-albums-title"
    >
      <h2 id="unmatched-albums-title">
        In your library, not matched to a release
      </h2>
      <span className="detail-section-note">
        Hito couldn't tell which release these are. Match one and it joins the
        discography above.
      </span>
      <ul className="release-grid">
        {releases.map((release) => (
          <li key={release.id} className="release-card">
            <Link
              className="release-tile"
              href={`/albums/${encodeURIComponent(release.id)}`}
              aria-label={`Open ${release.title}`}
            >
              <span className="release-tile-cover">
                <Artwork
                  release={release.id}
                  fallback={{
                    id: release.id,
                    title: release.title,
                    artist: release.artist,
                  }}
                />
              </span>
              <span className="release-tile-title">{release.title}</span>
              <span className="release-tile-subtitle">
                {release.track_count}{" "}
                {release.track_count === 1 ? "track" : "tracks"}
              </span>
            </Link>
          </li>
        ))}
      </ul>
    </section>
  );
}
