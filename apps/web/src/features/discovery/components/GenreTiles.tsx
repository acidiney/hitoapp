import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";
import CoverArtSvg from "../../../shared/ui/CoverArtSvg";
import { coverArt } from "../../../shared/ui/coverArt";
import { genreCount } from "../model/genreCount";
import { genreTitle } from "../model/genreTitle";

export const genrePath = (key: string) =>
  `/genres?genre=${encodeURIComponent(key)}`;

export default function GenreTiles({ genres }: { genres: Schema<"Genre">[] }) {
  if (genres.length === 0) return null;
  return (
    <section className="discover-section" aria-labelledby="discover-genres">
      <h2 id="discover-genres">By genre</h2>
      <ul className="genre-tiles">
        {genres.map((genre) => {
          const name = genreTitle(genre.label);
          return (
            <li key={genre.key}>
              <Link className="genre-tile" href={genrePath(genre.key)}>
                <CoverArtSvg art={coverArt("genre", genre.key, name, null)} />
                <span className="genre-tile-name">{name}</span>
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
