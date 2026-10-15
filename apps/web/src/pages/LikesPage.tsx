import type { CatalogPageProps } from "../types";
import CatalogResults from "../components/CatalogResults";
import PageHeading from "../components/PageHeading";

export default function LikesPage(props: CatalogPageProps) {
  return (
    <>
      <PageHeading
        title="Liked tracks"
        description={`${props.recordings.total} saved tracks`}
      />
      <CatalogResults
        {...props}
        path="/likes"
        emptyTitle="Your favorites belong here"
        emptyText="Choose music from your library to get started."
      />
    </>
  );
}
