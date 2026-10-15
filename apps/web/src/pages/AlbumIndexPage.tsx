import type { AlbumIndexPageProps } from "../types";
import PageHeading from "../components/PageHeading";
import AlbumGrid from "../components/AlbumGrid";
import EmptyState from "../components/EmptyState";
import Pagination from "../components/Pagination";

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
