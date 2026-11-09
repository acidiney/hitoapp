import type { Schema } from "../../../shared/schema";
import TrackTable, {
  type TrackTableRow,
} from "../../playback/components/TrackTable";
import { trackListing } from "../../playback/api/trackListing";
import TrackToolbar from "../../playback/components/TrackToolbar";

type Playlists = Parameters<typeof TrackTable>[0]["playlists"];
type Mutate = Parameters<typeof TrackTable>[0]["mutate"];

export default function ArtistAppearances({
  appearances,
  playlists,
  mutate,
}: {
  appearances: Schema<"Page_Recording">;
  playlists: Playlists;
  mutate: Mutate;
}) {
  if (appearances.items.length === 0) return null;
  const tracks = appearances.items.map((recording) => ({
    ...recording,
    row_id: recording.id,
  }));
  const shown = tracks.length;
  const listing = trackListing({ total: shown, offset: 0 }, async () => ({
    total: shown,
    items: tracks,
  }));
  return (
    <section aria-labelledby="artist-appearances-title">
      <h2 className="discography-heading" id="artist-appearances-title">
        Appears on
      </h2>
      <TrackToolbar
        tracks={tracks}
        listing={listing}
        withPlay={false}
        description="Recordings that credit this artist as a guest"
      />
      <TrackTable
        rows={tracks.map<TrackTableRow>((track) => ({
          kind: "recording",
          track,
        }))}
        listing={listing}
        offset={0}
        playlists={playlists}
        mutate={mutate}
      />
      {appearances.total > shown && (
        <p className="muted">
          Showing {shown} of {appearances.total} appearances
        </p>
      )}
    </section>
  );
}
