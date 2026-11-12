import type { ReactNode } from "react";
import { LyricsPanel } from "./NowPlayingLyrics";
import { SourcePanel } from "./NowPlayingAside";
import { PanelTabs, type PhonePanel } from "./NowPlayingPhone";
import type { Track } from "../model";

export default function NowPlayingTablet({
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
  panel: Exclude<PhonePanel, null>;
  choose: (panel: PhonePanel) => void;
  leave: () => void;
  openQueue: () => void;
  track: ReactNode;
}) {
  return (
    <>
      {track}
      <div className="np-tablet-side">
        <PanelTabs panel={panel} choose={choose} openQueue={openQueue} />
        {panel === "lyrics" ? (
          <LyricsPanel account={account} track={current} leave={leave} />
        ) : (
          <SourcePanel account={account} track={current} leave={leave} />
        )}
      </div>
    </>
  );
}
