import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";
import { Play } from "lucide-react";

import Artwork from "../../artwork/components/Artwork";
import Button from "../../../shared/ui/Button";
import { releaseCreditLabel } from "../../catalog/model/albumCredit";

const plural = (count: number) => (count === 1 ? "track" : "tracks");

export default function UnheardNearby({
  albums,
  onListen,
}: {
  albums: Schema<"UnheardOnAlbum">[];
  onListen: (album: Schema<"UnheardOnAlbum">) => void;
}) {
  return (
    <section className="unheard" aria-labelledby="unheard-heading">
      <h2 id="unheard-heading">Unheard, near what you like</h2>
      {albums.map((album) => (
        <article key={album.release_id} className="unheard-card">
          <Artwork
            release={album.release_id}
            fallback={{
              id: album.release_id,
              title: album.title,
              artist: releaseCreditLabel(album),
            }}
          />
          <div className="unheard-copy">
            <p className="unheard-why">
              You played {album.played} {plural(album.played)} from{" "}
              {album.title}.
            </p>
            <p className="unheard-what">
              {album.remaining} more {plural(album.remaining)} from{" "}
              {album.title}
            </p>
            <div className="unheard-actions">
              <Button
                variant="primary"
                aria-label={`Listen to ${album.title}`}
                onClick={() => onListen(album)}
              >
                <Play size={14} fill="currentColor" aria-hidden="true" />
                Listen
              </Button>
              <Link
                className="button secondary"
                href={`/albums/${album.release_id}`}
                aria-label={`Open ${album.title}`}
              >
                Open album
              </Link>
            </div>
          </div>
        </article>
      ))}
    </section>
  );
}
