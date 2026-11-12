import { Link } from "@inertiajs/react";
import StatusDot from "../../../shared/ui/StatusDot";
import type { Track } from "../model";
import { sourceLabels } from "../../sources/model/sourceLabels";
import { useRowSource } from "../hooks/useRowSource";
import { useTrackActions } from "./TrackActionsProvider";
import { sourceHref } from "./TrackRowMenu";

export default function TrackRowSource({
  track,
}: {
  track: Pick<Track, "id" | "available" | "plays_from_services"> | null;
}) {
  const account = useTrackActions()?.account ?? null;
  const resolved = useRowSource(
    account,
    track?.id ?? "",
    track === null || track.available || !!track.plays_from_services,
  );
  if (track === null) return <StatusDot status="none" />;
  if (track.available) return <StatusDot status="ready" detail="Local" />;
  if (resolved?.ready)
    return (
      <StatusDot
        status="ready"
        detail={
          resolved.provider === "local"
            ? "Local"
            : `from ${sourceLabels(resolved).primary}`
        }
      />
    );
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
