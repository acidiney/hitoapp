import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { Link } from "@inertiajs/react";
import { genreCount } from "../model/genreCount";
import { genreTitle } from "../model/genreTitle";

const PREVIEW = 12;
const GENRE_TONES = 6;

export const genrePath = (key: string) =>
  `/genres?genre=${encodeURIComponent(key)}`;

export default function GenreTiles({ genres }: { genres: Schema<"Genre">[] }) {
  const [all, setAll] = useState(false);
  if (genres.length === 0) return null;
  const shown = all ? genres : genres.slice(0, PREVIEW);
  return (
    <section className="discover-section" aria-labelledby="discover-genres">
      <div className="discover-section-head">
        <h2 id="discover-genres">Genres you own</h2>
        {genres.length > PREVIEW && (
          <button
            type="button"
            className="text-link"
            aria-expanded={all}
            onClick={() => setAll(!all)}
          >
            {all ? "Show fewer" : `Show all ${genres.length}`}
          </button>
        )}
      </div>
      <ul className="genre-tiles">
        {shown.map((genre, index) => (
          <li key={genre.key}>
            <Link
              className="genre-tile"
              data-tone={index % GENRE_TONES}
              href={genrePath(genre.key)}
            >
              <span className="genre-tile-name">{genreTitle(genre.label)}</span>
              <span className="genre-tile-count">
                {genreCount(genre.track_count)}{" "}
                {genre.track_count === 1 ? "track" : "tracks"}
              </span>
            </Link>
          </li>
        ))}
      </ul>
    </section>
  );
}
