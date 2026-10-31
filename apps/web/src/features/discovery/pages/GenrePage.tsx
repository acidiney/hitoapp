import type { Schema } from "../../../shared/schema";

import { useMutation } from "../../../shared/hooks/useMutation";
import AlbumGrid from "../../catalog/components/AlbumGrid";
import Breadcrumbs from "../../../shared/ui/Breadcrumbs";
import CollectionHeader from "../../../shared/ui/CollectionHeader";
import { tintFor } from "../../../shared/ui/collectionTint";
import PlayShuffle from "../../playback/components/PlayShuffle";
import { genreCount } from "../model/genreCount";
import TrackTable, {
  type TrackTableRow,
} from "../../playback/components/TrackTable";

const tracksIn = (count: number) =>
  `${genreCount(count)} ${count === 1 ? "track" : "tracks"} in your library`;

export default function GenrePage({
  user,
  playlists,
  genre: detail,
}: Schema<"GenrePageProps">) {
  const { genre, albums, tracks } = detail;
  const { mutate } = useMutation(user.id);
  const rows = tracks.map((track) => ({ ...track, row_id: track.id }));
  return (
    <>
      <CollectionHeader
        tint={tintFor(genre.key)}
        size="playlist"
        crumbs={
          <Breadcrumbs
            crumbs={[
              { label: "Discover", href: "/discover" },
              { label: genre.label },
            ]}
          />
        }
        eyebrow="Genre"
        title={genre.label}
        stats={tracksIn(genre.track_count)}
        actions={<PlayShuffle tracks={rows} label={`${genre.label} tracks`} />}
      />
      {albums.length > 0 && (
        <section aria-labelledby="genre-albums">
          <h2 className="discography-heading" id="genre-albums">
            Albums
          </h2>
          <AlbumGrid releases={albums} />
        </section>
      )}
      <section aria-labelledby="genre-tracks">
        <h2 className="discography-heading" id="genre-tracks">
          Tracks
        </h2>
        {genre.track_count > tracks.length && (
          <p className="muted">
            Showing {tracks.length} of {genreCount(genre.track_count)} tracks.
          </p>
        )}
        <TrackTable
          rows={rows.map<TrackTableRow>((track) => ({
            kind: "recording",
            track,
          }))}
          playlists={playlists}
          mutate={mutate}
        />
      </section>
    </>
  );
}
