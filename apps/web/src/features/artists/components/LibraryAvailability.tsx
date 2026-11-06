import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";
import AlbumLevelCheck from "./AlbumLevelCheck";

function tracks(count: number) {
  return `${count} ${count === 1 ? "track" : "tracks"}`;
}

export default function LibraryAvailability({
  account,
  albums,
  group,
}: {
  account: string;
  albums: Schema<"LibraryAlbum">[];
  group: string;
}) {
  if (albums.length === 0)
    return <p className="sr-only">Not linked to a library album</p>;
  return (
    <ul aria-label="In your library">
      {albums.map((album) => (
        <li key={album.release_id}>
          <p>
            {album.playable_count === 0
              ? `${tracks(album.track_count)} in your library, none playable yet`
              : album.playable_count === album.track_count
                ? `${tracks(album.track_count)} in your library, all playable`
                : `${album.playable_count} of ${album.track_count} tracks playable`}
          </p>
          <AlbumLevelCheck
            account={account}
            release={album.release_id}
            releaseGroup={group}
          />
          <Link
            className="text-link"
            href={`/albums/${encodeURIComponent(album.release_id)}`}
          >
            Open in your library
          </Link>
        </li>
      ))}
    </ul>
  );
}
