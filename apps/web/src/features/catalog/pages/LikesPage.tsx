import type { Schema } from "../../../shared/schema";

import { longDuration } from "../../../shared/format";
import CatalogResults from "../components/CatalogResults";

export default function LikesPage(props: Schema<"CatalogPageProps">) {
  const { items, total } = props.recordings;
  const loadedDuration =
    items.length === total && total > 0
      ? ` · ${longDuration(items.reduce((sum, item) => sum + item.duration_ms, 0) / 1000)}`
      : "";
  return (
    <>
      <CatalogResults
        {...props}
        title="Liked tracks"
        description={`${props.recordings.total} liked ${props.recordings.total === 1 ? "track" : "tracks"}${loadedDuration}`}
        path="/likes"
        emptyTitle="Your favorites belong here"
        emptyText="Choose music from your library to get started."
      />
    </>
  );
}
