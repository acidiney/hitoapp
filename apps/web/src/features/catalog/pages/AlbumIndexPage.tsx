import type { AlbumIndexPageProps } from "../../../shared/types";
import PageHeading from "../../../shared/ui/PageHeading";
import AlbumGrid from "../../../shared/ui/AlbumGrid";
import EmptyState from "../../../shared/ui/EmptyState";
import Pagination from "../../../shared/ui/Pagination";

export default function AlbumIndexPage({
  releases,
  query,
}: AlbumIndexPageProps) {
  return (
    <>
      <PageHeading
        title="Albums"
        description={`${releases.total} ${releases.total === 1 ? "album" : "albums"} in your libraries`}
      />
      {releases.items.length ? (
        <AlbumGrid releases={releases.items} />
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
      <Pagination page={releases} query={query} path="/albums" />
    </>
  );
}
