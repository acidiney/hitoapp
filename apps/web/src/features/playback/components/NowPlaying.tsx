import {
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type CSSProperties,
} from "react";
import { ChevronDown } from "lucide-react";
import { usePlayer } from "./PlaybackProvider";
import { coverPaint } from "../../../shared/ui/coverArt";
import { oklchToHex } from "../../../shared/ui/coverColor";
import NowPlayingAside from "./NowPlayingAside";
import { LyricsPanel } from "./NowPlayingLyrics";
import NowPlayingPhone, { type PhonePanel } from "./NowPlayingPhone";
import NowPlayingTablet from "./NowPlayingTablet";
import NowPlayingTrack from "./NowPlayingTrack";
import PlayerTrackActions from "./PlayerTrackActions";
import { useNowPlayingMotion } from "../hooks/useNowPlayingMotion";
import {
  PHONE_QUERY,
  TABLET_QUERY,
  useMediaQuery,
} from "../../../shared/hooks/useMediaQuery";
import type { Track } from "../model";

function roomTint(current: Track | null): CSSProperties {
  const hue = coverPaint(
    "album",
    current?.release_id ?? current?.id ?? "none",
  ).hue;
  return {
    "--np-tint": oklchToHex(0.82, 0.08, hue),
    "--np-past": oklchToHex(0.66, 0.03, hue),
    "--np-next": oklchToHex(0.8, 0.02, hue),
  } as CSSProperties;
}

export default function NowPlaying({
  account,
  close,
  openQueue,
}: {
  account: string;
  close: () => void;
  openQueue: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const phone = useMediaQuery(PHONE_QUERY);
  const tablet = useMediaQuery(TABLET_QUERY);
  const [panel, setPanel] = useState<PhonePanel>(null);
  const cover = useRef<HTMLDivElement>(null);
  const player = usePlayer();
  const current = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().current,
  );
  useLayoutEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);
  const motion = useNowPlayingMotion(dialog, cover, () =>
    dialog.current?.close(),
  );
  return (
    <dialog
      ref={dialog}
      className="now-playing-dialog"
      aria-label="Now playing"
      style={roomTint(current)}
      onClose={close}
      onCancel={(event) => {
        event.preventDefault();
        motion.requestClose();
      }}
    >
      {!(phone && panel) && (
        <header data-np-fade="">
          <button
            className="icon-button"
            aria-label="Close now playing"
            onClick={motion.requestClose}
          >
            <ChevronDown size={22} />
          </button>
          <span>Now playing</span>
          {current ? (
            <PlayerTrackActions
              track={current}
              liking={false}
              onNavigate={close}
            />
          ) : (
            <span aria-hidden="true" className="np-header-balance" />
          )}
        </header>
      )}
      <div
        className="np-body"
        data-panel={phone ? (panel ?? "player") : undefined}
      >
        {phone ? (
          <NowPlayingPhone
            account={account}
            current={current}
            panel={panel}
            choose={setPanel}
            leave={close}
            openQueue={openQueue}
            track={
              <NowPlayingTrack
                account={account}
                current={current}
                cover={cover}
                close={close}
              />
            }
          />
        ) : tablet ? (
          <NowPlayingTablet
            account={account}
            current={current}
            panel={panel === "source" ? "source" : "lyrics"}
            choose={(next) => setPanel(next ?? "lyrics")}
            leave={close}
            openQueue={openQueue}
            track={
              <NowPlayingTrack
                account={account}
                current={current}
                cover={cover}
                close={close}
              />
            }
          />
        ) : (
          <>
            <NowPlayingTrack
              account={account}
              current={current}
              cover={cover}
              close={close}
            />
            <LyricsPanel account={account} track={current} leave={close} />
            <NowPlayingAside
              account={account}
              track={current}
              openQueue={openQueue}
              leave={close}
            />
          </>
        )}
      </div>
    </dialog>
  );
}
