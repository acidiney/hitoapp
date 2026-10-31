import type { Schema } from "../../../shared/schema";
import type { CSSProperties } from "react";
import { Link } from "@inertiajs/react";
import { coverStyle } from "../../../shared/ui/coverPalette";
import { genreCount } from "../model/genreCount";

export const genrePath = (key: string) =>
  `/genres?genre=${encodeURIComponent(key)}`;

export default function GenreTiles({ genres }: { genres: Schema<"Genre">[] }) {
  if (genres.length === 0) return null;
  return (
    <section className="discover-section" aria-labelledby="discover-genres">
      <h2 id="discover-genres">By genre</h2>
      <ul className="genre-tiles">
        {genres.map((genre) => {
          const colors = coverStyle(genre.key);
          return (
            <li key={genre.key}>
              <Link
                className="genre-tile"
                href={genrePath(genre.key)}
                style={
                  {
                    "--tile": colors.background,
                    "--tile-ink": colors.foreground,
                  } as CSSProperties
                }
              >
                <span className="genre-tile-name">{genre.label}</span>
                <span className="genre-tile-count">
                  {genreCount(genre.track_count)}{" "}
                  {genre.track_count === 1 ? "track" : "tracks"}
                </span>
              </Link>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
