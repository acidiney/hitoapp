import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";
import { FolderPlus, FolderSync } from "lucide-react";

import { useScanJob } from "../../library/hooks/useScanJob";
import CatalogResults from "../components/CatalogResults";
import PageHeading from "../../../shared/ui/PageHeading";
import LibrarySpotlight from "../components/LibrarySpotlight";
import Button from "../../../shared/ui/Button";

export default function CatalogPage(props: Schema<"CatalogPageProps">) {
  const { scan, scanning } = useScanJob(props.user.id, props.libraries[0]?.id);
  const description = `${props.recordings.total.toLocaleString()} ${props.recordings.total === 1 ? "track" : "tracks"}${props.query ? ` matching “${props.query}”` : ""}`;
  return (
    <>
      <PageHeading title="Your library" description={description}>
        {props.user.admin &&
          props.libraries.length > 0 &&
          (props.recordings.total === 0 && !props.query ? (
            <Link className="button primary" href="/settings">
              <FolderPlus size={17} />
              Add music folder
            </Link>
          ) : (
            <Button disabled={scanning} onClick={() => void scan()}>
              <FolderSync size={17} />
              {scanning ? "Scanning music…" : "Scan music"}
            </Button>
          ))}
      </PageHeading>
      {!props.query && props.recordings.offset === 0 && (
        <LibrarySpotlight
          tracks={props.recordings.items}
          playlists={props.playlists}
        />
      )}
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
            ? "Add your music folders in Settings. Hito will scan them to fill your library."
            : "Ask your administrator to give you access to a music library."
        }
      />
    </>
  );
}
