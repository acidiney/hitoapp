import { Link } from "@inertiajs/react";
import Artwork from "../../artwork/components/Artwork";
import FallbackCover from "../../../shared/ui/FallbackCover";
import type { QuickTile } from "../quickAccess";

function TileCover({ tile }: { tile: QuickTile }) {
  const fallback = {
    id: tile.coverId,
    title: tile.title,
    artist: tile.coverArtist,
  };
  if (tile.kind === "likes") return <FallbackCover {...fallback} />;
  return tile.kind === "release" ? (
    <Artwork release={tile.coverId} fallback={fallback} />
  ) : (
    <Artwork playlistId={tile.coverId} playlist fallback={fallback} />
  );
}

export default function QuickAccess({ tiles }: { tiles: QuickTile[] }) {
  return (
    <section className="quick-access" aria-label="Quick access">
      {tiles.map((tile) => (
        <Link key={tile.key} href={tile.href} className="quick-tile">
          <span className="quick-tile-cover">
            <TileCover tile={tile} />
          </span>
          <span className="quick-tile-copy">
            <span className="quick-tile-title">{tile.title}</span>
            <span className="quick-tile-kind">{tile.subtitle}</span>
          </span>
        </Link>
      ))}
    </section>
  );
}
