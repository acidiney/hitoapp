import type { CatalogPageProps } from "../../../shared/types";
import { useMutation } from "../../../shared/hooks/useMutation";
import EmptyState from "../../../shared/ui/EmptyState";
import Pagination from "../../../shared/ui/Pagination";
import TrackTable, {
  type TrackTableRow,
} from "../../playback/components/TrackTable";
import TrackToolbar from "../../playback/components/TrackToolbar";
import { useState } from "react";
import { Grid2X2, List } from "lucide-react";
import TrackGrid from "../../playback/components/TrackGrid";
import { log } from "../../../shared/log";

export default function CatalogResults({
  user,
  playlists,
  query,
  recordings,
  path,
  emptyTitle,
  emptyText,
  browse = false,
}: Pick<CatalogPageProps, "user" | "playlists" | "query" | "recordings"> & {
  path: string;
  emptyTitle: string;
  emptyText: string;
  browse?: boolean;
}) {
  const { mutate } = useMutation(user.id);
  const [view, setView] = useState<"grid" | "list">(browse ? "grid" : "list");
  function changeView(mode: "grid" | "list") {
    log.debug("catalog.view", { mode, account_id: user.id });
    setView(mode);
  }
  const tracks = recordings.items.map((track) => ({
    ...track,
    row_id: track.id,
  }));
  return (
    <>
      {tracks.length > 0 ? (
        <>
          <TrackToolbar
            tracks={tracks}
            description={
              recordings.total > recordings.limit
                ? `Showing ${recordings.offset + 1}–${recordings.offset + tracks.length}`
                : "Original audio"
            }
          >
            <div className="view-toggle" role="group" aria-label="Library view">
              <button
                className="icon-button"
                aria-label="Grid view"
                aria-pressed={view === "grid"}
                onClick={() => changeView("grid")}
              >
                <Grid2X2 size={18} />
              </button>
              <button
                className="icon-button"
                aria-label="List view"
                aria-pressed={view === "list"}
                onClick={() => changeView("list")}
              >
                <List size={19} />
              </button>
            </div>
          </TrackToolbar>
          {view === "grid" ? (
            <TrackGrid tracks={tracks} playlists={playlists} mutate={mutate} />
          ) : (
            <TrackTable
              rows={tracks.map<TrackTableRow>((track) => ({
                kind: "recording",
                track,
              }))}
              offset={recordings.offset}
              playlists={playlists}
              mutate={mutate}
            />
          )}
        </>
      ) : (
        <EmptyState
          title={query ? "No matching tracks" : emptyTitle}
          text={query ? "Try another title or artist." : emptyText}
          libraryLink={path !== "/library"}
        />
      )}
      <Pagination page={recordings} query={query} path={path} />
    </>
  );
}
