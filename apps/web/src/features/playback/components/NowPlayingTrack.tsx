import { useRef } from "react";
import type { Track } from "../model";
import Artwork from "../../artwork/components/Artwork";
import { coverIdentity } from "../../artwork/model/coverIdentity";
import { SeekBar, TransportButtons } from "./PlaybackControls";
import PlaybackStatus from "./PlaybackStatus";
import NowPlayingLike from "./NowPlayingLike";
import StatusDot from "../../../shared/ui/StatusDot";
import { ArtistNames, artistHref } from "./TrackNames";
import { artistSearchLink } from "../hooks/useArtistLink";
import { usePlayingSource } from "../hooks/usePlayingSource";
import { sourceLabels } from "../../sources/model/sourceLabels";

function SourceLine({ account, track }: { account: string; track: Track }) {
  const playing = usePlayingSource(account, track.id);
  if (playing.loading) return <StatusDot status="checking" />;
  if (!playing.source) return <StatusDot status="none" />;
  const labels = sourceLabels(playing.source);
  const where =
    playing.source.provider === "local" ? "Local" : `from ${labels.primary}`;
  return (
    <StatusDot
      status={playing.source.ready ? "ready" : "checking"}
      detail={where}
    />
  );
}

export default function NowPlayingTrack({
  account,
  current,
  cover,
  close,
}: {
  account: string;
  current: Track | null;
  cover: React.RefObject<HTMLDivElement | null>;
  close: () => void;
}) {
  const title = useRef<HTMLHeadingElement>(null);
  return (
    <section className="np-track" aria-label="Track">
      <div className="np-cover" ref={cover}>
        <Artwork
          release={current?.release_id ?? undefined}
          recording={current?.id}
          fallback={coverIdentity(current)}
        />
      </div>
      <div className="np-title" data-np-fade="">
        <div className="np-title-copy">
          <h1 ref={title}>{current?.title ?? "Ready when you are"}</h1>
          {current ? (
            <>
              <p className="np-artist">
                <ArtistNames
                  artist={current.artist}
                  artistHref={
                    artistHref(current) ?? artistSearchLink(current.artist)
                  }
                  credited={current.credited_artists}
                  routed={{ className: "np-artist-link", onNavigate: close }}
                />
              </p>
              <p className="np-source-line">
                <SourceLine account={account} track={current} />
              </p>
            </>
          ) : (
            <p className="np-artist">Choose a track to start listening.</p>
          )}
        </div>
        {current && <NowPlayingLike track={current} />}
      </div>
      <div className="np-seek" data-np-fade="">
        <SeekBar remaining />
      </div>
      <div className="np-transport" data-np-fade="">
        <TransportButtons />
      </div>
      <PlaybackStatus className="now-playing-status" />
    </section>
  );
}
