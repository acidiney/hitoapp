import type { Schema } from "../../../shared/schema";
import LoadMore from "../../../shared/ui/LoadMore";
import { useInfinitePage } from "../../../shared/hooks/useInfinitePage";
import TrackTable, {
  type TrackTableRow,
} from "../../playback/components/TrackTable";
import TrackToolbar from "../../playback/components/TrackToolbar";
import {
  artistAppearancesListing,
  artistAppearancesPage,
} from "../api/trackListings";

type Playlists = Parameters<typeof TrackTable>[0]["playlists"];
type Mutate = Parameters<typeof TrackTable>[0]["mutate"];

export default function ArtistAppearances({
  account,
  artist,
  appearances,
  playlists,
  mutate,
}: {
  account: string;
  artist: string;
  appearances: Schema<"Page_Recording">;
  playlists: Playlists;
  mutate: Mutate;
}) {
  const loaded = useInfinitePage({
    memory: `artist-appearances-${artist}`,
    scope: artist,
    initial: appearances,
    fetchPage: (offset, signal) =>
      artistAppearancesPage(account, artist, offset, appearances.limit, signal),
  });
  if (loaded.items.length === 0) return null;
  const tracks = loaded.items.map((recording) => ({
    ...recording,
    row_id: recording.id,
  }));
  const listing = artistAppearancesListing(account, artist, {
    total: appearances.total,
    offset: 0,
  });
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
      <LoadMore
        {...loaded}
        loaded={loaded.items.length}
        noun="appearances"
        onLoad={() => void loaded.loadMore()}
      />
    </section>
  );
}
