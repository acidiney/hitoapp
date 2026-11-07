import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";
import { FolderPlus, FolderSync } from "lucide-react";

import { useScanJob } from "../../library/hooks/useScanJob";
import CatalogResults from "../components/CatalogResults";
import PageHeading from "../../../shared/ui/PageHeading";
import Button from "../../../shared/ui/Button";
import { libraryDescription } from "../model/libraryCounts";

export default function CatalogPage(props: Schema<"CatalogPageProps">) {
  const { scan, scanning } = useScanJob(props.user.id, props.libraries[0]?.id);
  const scanLabel = scanning ? "Scanning music…" : "Scan music";
  const canScan =
    props.user.admin &&
    props.libraries.length > 0 &&
    (props.recordings.total > 0 || Boolean(props.query));
  return (
    <>
      <PageHeading
        title="Your library"
        description={libraryDescription({
          total: props.recordings.total,
          files: props.files,
          query: props.query,
        })}
      >
        {canScan && (
          <Button
            className="heading-action"
            aria-label={scanLabel}
            disabled={scanning}
            onClick={() => void scan()}
          >
            <FolderSync size={17} />
            <span className="button-label">{scanLabel}</span>
          </Button>
        )}
      </PageHeading>
      <CatalogResults
        {...props}
        path="/library"
        browse
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
