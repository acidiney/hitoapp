import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";
import { FolderPlus, FolderSync } from "lucide-react";

import { useScanJob } from "../../library/hooks/useScanJob";
import CatalogResults from "../components/CatalogResults";
import Button from "../../../shared/ui/Button";
import { libraryDescription } from "../model/libraryCounts";
import {
  TRACK_SORTS,
  useCatalogSort,
} from "../../../shared/hooks/useCatalogSort";

export default function CatalogPage(props: Schema<"CatalogPageProps">) {
  const order = useCatalogSort("/library", TRACK_SORTS);
  const shared = props.libraries.find((library) => !library.personal);
  const { scan, scanning } = useScanJob(props.user.id, shared?.id);
  const scanLabel = scanning ? "Scanning music…" : "Scan music";
  const canScan =
    props.user.admin &&
    shared !== undefined &&
    (props.recordings.total > 0 || Boolean(props.query));
  return (
    <>
      <CatalogResults
        {...props}
        title="Your library"
        description={libraryDescription({
          total: props.recordings.total,
          files: props.files,
          query: props.query,
        })}
        headingActions={
          canScan && (
            <Button
              className="heading-action"
              aria-label={scanLabel}
              disabled={scanning}
              onClick={() => void scan()}
            >
              <FolderSync size={17} />
              <span className="button-label">{scanLabel}</span>
            </Button>
          )
        }
        path="/library"
        browse
        sort={{
          value: order.sort,
          options: order.options,
          onChange: order.change,
        }}
        emptyTitle={
          props.user.admin
            ? "Bring your music along"
            : "Your library is waiting"
        }
        emptyText={
          props.user.admin
            ? "Hito reads the music in the folders you add and fills your library with it."
            : "An administrator adds the music folders. Your library fills in once they have."
        }
        emptyAction={
          props.user.admin && (
            <Link className="button primary" href="/settings?section=music">
              <FolderPlus size={17} />
              Add a music folder
            </Link>
          )
        }
      />
    </>
  );
}
