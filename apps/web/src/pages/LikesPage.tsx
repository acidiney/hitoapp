import type { CatalogPageProps } from "../shared/types";
import CatalogResults from "../components/CatalogResults";
import PageHeading from "../shared/ui/PageHeading";

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
