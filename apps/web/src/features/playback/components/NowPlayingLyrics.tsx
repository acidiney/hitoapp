import { trackTabHref } from "../../recordings/model/trackTabs";
import { useState } from "react";
import type { Track } from "../model";
import { useNowPlayingLyrics } from "../../lyrics/hooks/useNowPlayingLyrics";
import RouteLink from "../../../shared/ui/RouteLink";
import FilterPills from "../../../shared/ui/FilterPills";
import { useAccountSwitch } from "../../../shared/hooks/useAccountSwitch";

function TrackLyrics({
  account,
  track,
  leave,
}: {
  account: string;
  track: Track;
  leave: () => void;
}) {
  const [automatic] = useAccountSwitch("lyrics-auto", account);
  const { state, choose, retry } = useNowPlayingLyrics(
    account,
    track.id,
    automatic,
  );
  const [choosing, setChoosing] = useState(false);
  const details = (
    <RouteLink href={trackTabHref(track.id, "lyrics")} onNavigate={leave}>
      Track details
    </RouteLink>
  );
  if (state.status === "idle")
    return (
      <>
        <button type="button" className="text-link" onClick={retry}>
          Load lyrics
        </button>
        <p className="muted">
          Lyrics are loaded from your lyrics provider when you ask.
        </p>
        {details}
      </>
    );
  if (state.status === "loading")
    return (
      <p role="status" className="muted">
        Loading lyrics…
      </p>
    );
  if (state.status === "no_provider")
    return (
      <>
        <p>No lyrics provider connected</p>
        <RouteLink href="/settings?section=plugins" onNavigate={leave}>
          Connect one in Settings
        </RouteLink>
      </>
    );
  if (state.status === "failed")
    return (
      <>
        <p role="alert">Lyrics could not be loaded.</p>
        <button type="button" className="text-link" onClick={retry}>
          Try again
        </button>
      </>
    );
  if (state.status === "none_for_track")
    return (
      <>
        <p>No lyrics found for this track.</p>
        {details}
      </>
    );
  const { lyrics, provider, providers } = state;
  return (
    <>
      {lyrics ? (
        <ol className="np-lyric-lines" aria-label="Lyric lines">
          {lyrics.lines.map((line, index) => (
            <li key={index}>{line.text}</li>
          ))}
        </ol>
      ) : (
        <p>
          {state.unsupported
            ? "This provider has no lyrics for this track."
            : "No lyrics found for this track."}
        </p>
      )}
      <p className="muted">Lyrics from {provider.label}</p>
      {providers.length > 1 && !choosing && (
        <button
          type="button"
          className="text-link"
          onClick={() => setChoosing(true)}
        >
          Choose another provider
        </button>
      )}
      {choosing && (
        <FilterPills
          label="Lyric provider"
          options={providers.map((item) => ({
            value: item.observation_id,
            label: item.label,
          }))}
          value={provider.observation_id}
          onChange={(observation) => {
            setChoosing(false);
            void choose(observation);
          }}
        />
      )}
      {details}
    </>
  );
}

export function LyricsPanel({
  account,
  track,
  leave,
}: {
  account: string;
  track: Track | null;
  leave: () => void;
}) {
  return (
    <div className="np-lyrics">
      {track ? (
        <TrackLyrics
          key={track.id}
          account={account}
          track={track}
          leave={leave}
        />
      ) : (
        <p className="muted">Play a track to see its lyrics.</p>
      )}
    </div>
  );
}
