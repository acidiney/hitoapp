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
    <section className="unheard" aria-label="Unheard, near what you like">
      {albums.map((album) => (
        <article key={album.release_id} className="unheard-card">
          <div className="unheard-cover">
            <Artwork
              release={album.release_id}
              fallback={{
                id: album.release_id,
                title: album.title,
                artist: releaseCreditLabel(album),
              }}
            />
          </div>
          <div className="unheard-copy">
            <p className="unheard-eyebrow">Unheard, near what you like</p>
            <h2 className="unheard-what">
              {album.remaining} more {plural(album.remaining)} from{" "}
              {album.title}
            </h2>
            <p className="unheard-why">
              You played {album.played} {plural(album.played)} from this album.
              These are the ones you haven&apos;t heard yet.
            </p>
            <div className="unheard-actions">
              <Button
                variant="action"
                aria-label={`Listen to ${album.title}`}
                onClick={() => onListen(album)}
              >
                <Play size={16} fill="currentColor" aria-hidden="true" />
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
