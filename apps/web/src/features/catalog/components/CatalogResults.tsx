import type { Schema } from "../../../shared/schema";

import { useMutation } from "../../../shared/hooks/useMutation";
import EmptyState from "../../../shared/ui/EmptyState";
import Pagination from "../../../shared/ui/Pagination";
import TrackTable, {
  type TrackTableRow,
} from "../../playback/components/TrackTable";
import PlayShuffle from "../../playback/components/PlayShuffle";
import LibraryHeading from "../../../app/LibraryHeading";
import SelectionBar from "./SelectionBar";
import { useTrackSelection } from "../hooks/useTrackSelection";
import { useState, type ReactNode } from "react";
import { Grid2X2, List } from "lucide-react";
import TrackGrid from "../../playback/components/TrackGrid";
import { log } from "../../../shared/log";
import { recordingsListing } from "../api/trackListings";
import SortControl from "../../../shared/ui/SortControl";
import SortSheet from "./SortSheet";
import {
  PHONE_QUERY,
  useMediaQuery,
} from "../../../shared/hooks/useMediaQuery";
import type { SortOption } from "../../../shared/hooks/useCatalogSort";

export default function CatalogResults({
  title,
  description,
  headingActions,
  user,
  playlists,
  query,
  recordings,
  path,
  emptyTitle,
  emptyText,
  emptyAction,
  browse = false,
  sort,
}: Pick<
  Schema<"CatalogPageProps">,
  "user" | "playlists" | "query" | "recordings"
> & {
  title: string;
  description: string;
  headingActions?: ReactNode;
  path: string;
  emptyTitle: string;
  emptyText: string;
  emptyAction?: ReactNode;
  browse?: boolean;
  sort?: {
    value: string;
    options: SortOption[];
    onChange: (value: string) => void;
  };
}) {
  const { mutate } = useMutation(user.id);
  const rowIds = recordings.items.map((track) => track.id);
  const selection = useTrackSelection(rowIds);
  const [view, setView] = useState<"grid" | "list">("list");
  function changeView(mode: "grid" | "list") {
    log.debug("catalog.view", { mode, account_id: user.id });
    setView(mode);
  }
  const phone = useMediaQuery(PHONE_QUERY);
  const sortControl =
    sort &&
    (phone ? (
      <SortSheet
        options={sort.options}
        value={sort.value}
        onChange={sort.onChange}
      />
    ) : (
      <SortControl
        options={sort.options}
        value={sort.value}
        onChange={sort.onChange}
      />
    ));
  const tracks = recordings.items.map((track) => ({
    ...track,
    row_id: track.id,
  }));
  const listing = recordingsListing(
    user.id,
    recordings,
    query ?? "",
    path === "/likes",
  );
  return (
    <>
      <LibraryHeading path={path} title={title} description={description}>
        <div className="library-heading-actions">
          {headingActions}
          {phone && tracks.length > 0 && sortControl}
          {tracks.length > 0 && (
            <PlayShuffle
              tracks={tracks}
              listing={listing}
              label="tracks"
              tone="plain"
            />
          )}
        </div>
      </LibraryHeading>
      {tracks.length > 0 ? (
        <>
          <div
            role="group"
            aria-label="Sort tracks"
            className="library-filters"
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
            {!phone && sortControl}
          </div>
          <SelectionBar
            account={user.id}
            playlists={playlists}
            tracks={tracks}
            selection={selection}
          />
          {view === "grid" ? (
            <TrackGrid
              tracks={tracks}
              listing={listing}
              playlists={playlists}
              mutate={mutate}
            />
          ) : (
            <TrackTable
              rows={tracks.map<TrackTableRow>((track) => ({
                kind: "recording",
                track,
              }))}
              listing={listing}
              offset={recordings.offset}
              layout="library"
              selection={browse ? selection : undefined}
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
        >
          {!query && emptyAction}
        </EmptyState>
      )}
      <Pagination page={recordings} query={query} path={path} />
    </>
  );
}
