import { Link } from "@inertiajs/react";
import type { Schema } from "../../../shared/schema";

import { useMutation } from "../../../shared/hooks/useMutation";
import EmptyState from "../../../shared/ui/EmptyState";
import PageHeading from "../../../shared/ui/PageHeading";
import TrackTable, {
  type TrackTableRow,
} from "../../playback/components/TrackTable";
import TrackToolbar from "../../playback/components/TrackToolbar";
import { setting } from "../../settings/model/settingsCatalog";
import { settingHref } from "../../settings/model/settingsSearch";

export default function HistoryPage({
  user,
  playlists,
  activity,
  history_enabled,
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
          {!history_enabled && (
            <p className="muted" role="status">
              Listening history is paused, so new plays are not added.{" "}
              <Link
                className="text-link"
                href={settingHref(setting("listening-history"))}
              >
                Open listening history settings
              </Link>
            </p>
          )}
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
      ) : history_enabled ? (
        <EmptyState
          title="Start listening"
          text="Choose music from your library to get started."
        />
      ) : (
        <EmptyState
          title="Listening history is paused"
          text="New plays are not being saved. Turn history back on in Settings to see what you play here."
          libraryLink={false}
        >
          <Link
            className="button"
            href={settingHref(setting("listening-history"))}
          >
            Open listening history settings
          </Link>
        </EmptyState>
      )}
    </>
  );
}
