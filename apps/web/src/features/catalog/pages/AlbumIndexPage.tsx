import type { Schema } from "../../../shared/schema";

import PageHeading from "../../../shared/ui/PageHeading";
import AlbumGrid from "../components/AlbumGrid";
import EmptyState from "../../../shared/ui/EmptyState";
import LoadMore from "../../../shared/ui/LoadMore";
import { useInfinitePage } from "../../../shared/hooks/useInfinitePage";
import { albumListApi } from "../api/albumListApi";
import SortControl from "../../../shared/ui/SortControl";
import {
  ALBUM_SORTS,
  useCatalogSort,
} from "../../../shared/hooks/useCatalogSort";

export default function AlbumIndexPage({
  user,
  releases,
  query,
}: Schema<"AlbumIndexPageProps">) {
  const order = useCatalogSort("/albums", ALBUM_SORTS);
  const loaded = useInfinitePage({
    memory: "albums",
    scope: JSON.stringify({ query, sort: order.sort }),
    initial: releases,
    fetchPage: (offset, signal) =>
      albumListApi.releases(
        user.id,
        { query, sort: order.sort },
        offset,
        signal,
      ),
  });
  return (
    <>
      <PageHeading
        title="Albums"
        description={`${releases.total} ${releases.total === 1 ? "album" : "albums"}`}
      >
        <SortControl
          options={order.options}
          value={order.sort}
          onChange={order.change}
        />
      </PageHeading>
      {loaded.items.length ? (
        <AlbumGrid releases={loaded.items} />
      ) : (
        <EmptyState
          title={query ? "No matching albums" : "Your albums will be here"}
          text={
            query
              ? "Try another album or artist name."
              : "Albums come from the tags in your music folders."
          }
          libraryLink
        />
      )}
      <LoadMore
        {...loaded}
        loaded={loaded.items.length}
        noun="albums"
        onLoad={() => void loaded.loadMore()}
      />
    </>
  );
}
