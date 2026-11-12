import type { Schema } from "../../../shared/schema";

import { useMutation } from "../../../shared/hooks/useMutation";
import AlbumGrid from "../../catalog/components/AlbumGrid";
import Breadcrumbs from "../../../shared/ui/Breadcrumbs";
import GenreCover from "../components/GenreCover";
import GenreHeader from "../components/GenreHeader";
import PlayShuffle from "../../playback/components/PlayShuffle";
import { genreCount } from "../model/genreCount";
import { genreTitle } from "../model/genreTitle";
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
  const name = genreTitle(genre.label);
  const { mutate } = useMutation(user.id);
  const rows = tracks.map((track) => ({ ...track, row_id: track.id }));
  return (
    <>
      <GenreHeader
        cover={<GenreCover albums={albums} title={name} />}
        crumbs={
          <Breadcrumbs
            crumbs={[{ label: "Discover", href: "/discover" }, { label: name }]}
          />
        }
        title={name}
        stats={tracksIn(genre.track_count)}
        actions={
          <PlayShuffle tracks={rows} label={`${name} tracks`} tone="header" />
        }
      />
      {albums.length > 0 && (
        <section className="genre-section" aria-labelledby="genre-albums">
          <h2 id="genre-albums">Albums</h2>
          <AlbumGrid releases={albums} />
        </section>
      )}
      <section className="genre-section" aria-labelledby="genre-tracks">
        <h2 id="genre-tracks">Tracks</h2>
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
