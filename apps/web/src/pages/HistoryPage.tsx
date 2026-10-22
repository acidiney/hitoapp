import type { HistoryPageProps } from "../shared/types";
import { useMutation } from "../shared/hooks/useMutation";
import EmptyState from "../shared/ui/EmptyState";
import PageHeading from "../shared/ui/PageHeading";
import TrackTable, { type TrackTableRow } from "../shared/ui/TrackTable";
import TrackToolbar from "../shared/ui/TrackToolbar";

export default function HistoryPage({
  user,
  playlists,
  activity,
}: HistoryPageProps) {
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
        description="Pick up where you left off."
      />
      {tracks.length > 0 ? (
        <>
          <TrackToolbar tracks={tracks} description="Original audio" />
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
