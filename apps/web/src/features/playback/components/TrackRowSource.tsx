import { Link } from "@inertiajs/react";
import StatusDot from "../../../shared/ui/StatusDot";
import type { Track } from "../model";
import { sourceHref } from "./TrackRowMenu";

export default function TrackRowSource({
  track,
}: {
  track: Pick<Track, "id" | "available" | "plays_from_services"> | null;
}) {
  if (track === null) return <StatusDot status="none" />;
  if (track.available) return <StatusDot status="ready" detail="Local" />;
  if (track.plays_from_services)
    return <StatusDot status="ready" detail="from your services" />;
  return (
    <>
      <StatusDot status="none" />
      <Link className="source-find" href={sourceHref(track)}>
        Find
      </Link>
    </>
  );
}
