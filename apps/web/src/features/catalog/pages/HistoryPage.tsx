import type { Schema } from "../../../shared/schema";

import { useMutation } from "../../../shared/hooks/useMutation";
import EmptyState from "../../../shared/ui/EmptyState";
import PageHeading from "../../../shared/ui/PageHeading";
import TrackTable, {
  type TrackTableRow,
} from "../../playback/components/TrackTable";
import TrackToolbar from "../../playback/components/TrackToolbar";

export default function HistoryPage({
  user,
  playlists,
  activity,
}: Schema<"HistoryPageProps">) {
  const { mutate } = useMutation(user.id);
  const tracks = activity.history.map((track) => ({
    ...track,
    row_id: track.id,
    playback_start: {
      kind: "saved" as const,
      position: track.position_ms / 1000,
    },
  }));
  return (
    <>
      <PageHeading
        title="Listening history"
        description={`${tracks.length} recently played ${tracks.length === 1 ? "track" : "tracks"}`}
      />
      {tracks.length > 0 ? (
        <>
          <TrackToolbar tracks={tracks} />
          <TrackTable
            rows={tracks.map<TrackTableRow>((track) => ({
              kind: "recording",
              track,
            }))}
            playlists={playlists}
            mutate={mutate}
          />
        </>
      ) : (
        <EmptyState
          title="Start listening"
          text="Choose music from your library to get started."
        />
      )}
    </>
  );
}
