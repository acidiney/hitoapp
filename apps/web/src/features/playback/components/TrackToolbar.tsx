import type { ReactNode } from "react";
import { Play } from "lucide-react";
import { usePlayer } from "./PlaybackProvider";
import type { Track } from "../playback";

export default function TrackToolbar({
  tracks,
  label = "tracks",
  description,
  children,
}: {
  tracks: Track[];
  label?: string;
  description: string;
  children?: ReactNode;
}) {
  const player = usePlayer();
  return (
    <div className="list-toolbar">
      <button
        className="button primary"
        disabled={tracks.length === 0}
        onClick={() => void player.playQueue(tracks)}
      >
        <Play size={17} />
        Play {label}
      </button>
      <span className="muted">{description}</span>
      {children}
    </div>
  );
}
