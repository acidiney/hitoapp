import { useSyncExternalStore } from "react";
import { ChevronDown, Pause, Play } from "lucide-react";
import Artwork from "../../artwork/components/Artwork";
import { coverIdentity } from "../../artwork/model/coverIdentity";
import { SeekBar } from "./PlaybackControls";
import { usePlayer } from "./PlaybackProvider";
import { LyricsPanel } from "./NowPlayingLyrics";
import { SourcePanel } from "./NowPlayingAside";
import type { Track } from "../model";

export type PhonePanel = "lyrics" | "source" | null;

function CompactTrack({
  current,
  back,
}: {
  current: Track | null;
  back: () => void;
}) {
  const player = usePlayer();
  const state = useSyncExternalStore(player.subscribe, player.snapshot);
  const playing = state.desiredPlaying ?? state.playing;
  return (
    <>
      <div className="np-compact">
        <button
          type="button"
          className="icon-button"
          aria-label="Close panel, back to player"
          onClick={back}
        >
          <ChevronDown size={22} aria-hidden="true" />
        </button>
        <span className="np-compact-cover" aria-hidden="true">
          <Artwork
            release={current?.release_id ?? undefined}
            recording={current?.id}
            fallback={coverIdentity(current)}
          />
        </span>
        <span className="np-compact-copy">
          <strong>{current?.title ?? "Ready when you are"}</strong>
          <small>
            {current?.artist ?? "Choose a track to start listening."}
          </small>
        </span>
        <button
          type="button"
          className="np-compact-play"
          aria-label={playing ? "Pause" : "Play"}
          disabled={!current || state.connection !== "ready"}
          onClick={() => void player.toggle()}
        >
          {playing ? (
            <Pause size={18} fill="currentColor" aria-hidden="true" />
          ) : (
            <Play size={18} fill="currentColor" aria-hidden="true" />
          )}
        </button>
      </div>
      <div className="np-compact-seek">
        <SeekBar />
      </div>
    </>
  );
}

function PanelTabs({
  panel,
  choose,
  openQueue,
}: {
  panel: PhonePanel;
  choose: (panel: PhonePanel) => void;
  openQueue: () => void;
}) {
  const toggle = (next: Exclude<PhonePanel, null>) =>
    choose(panel === next ? null : next);
  return (
    <div className="np-panels" role="group" aria-label="Now playing panels">
      <button
        type="button"
        aria-pressed={panel === "lyrics"}
        onClick={() => toggle("lyrics")}
      >
        Lyrics
      </button>
      <button type="button" onClick={openQueue}>
        Up next
      </button>
      <button
        type="button"
        aria-pressed={panel === "source"}
        onClick={() => toggle("source")}
      >
        Source
      </button>
    </div>
  );
}

export default function NowPlayingPhone({
  account,
  current,
  panel,
  choose,
  leave,
  openQueue,
  track,
}: {
  account: string;
  current: Track | null;
  panel: PhonePanel;
  choose: (panel: PhonePanel) => void;
  leave: () => void;
  openQueue: () => void;
  track: React.ReactNode;
}) {
  if (panel === null)
    return (
      <>
        {track}
        <PanelTabs panel={panel} choose={choose} openQueue={openQueue} />
      </>
    );
  return (
    <>
      <CompactTrack current={current} back={() => choose(null)} />
      <PanelTabs panel={panel} choose={choose} openQueue={openQueue} />
      {panel === "lyrics" ? (
        <LyricsPanel account={account} track={current} leave={leave} />
      ) : (
        <SourcePanel account={account} track={current} leave={leave} />
      )}
    </>
  );
}
