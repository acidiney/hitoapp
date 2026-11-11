import { useState } from "react";
import type { Track } from "../model";
import { trackTabHref } from "../../recordings/model/trackTabs";
import { useNowPlayingLyrics } from "../../lyrics/hooks/useNowPlayingLyrics";
import { isSynced } from "../../lyrics/model/lineTiming";
import RouteLink from "../../../shared/ui/RouteLink";
import FilterPills from "../../../shared/ui/FilterPills";
import { useAccountSwitch } from "../../../shared/hooks/useAccountSwitch";
import { PlainLines, SyncedLines } from "./LyricLines";

type Lyrics = ReturnType<typeof useNowPlayingLyrics>;

function StateCard({
  tone,
  label,
  busy,
  title,
  children,
}: {
  tone?: "none" | "failed" | "checking";
  label: string;
  busy?: boolean;
  title?: string;
  children?: React.ReactNode;
}) {
  return (
    <section
      className="np-lyric-card"
      aria-label={label}
      aria-busy={busy || undefined}
      role={tone === "failed" ? "alert" : undefined}
    >
      <span className="np-lyric-label" data-tone={tone}>
        {tone && <span className="np-lyric-dot" aria-hidden="true" />}
        {label}
      </span>
      {title && <p className="np-lyric-title">{title}</p>}
      {children}
    </section>
  );
}

function Skeleton() {
  return (
    <StateCard tone="checking" label="Looking up lyrics…" busy>
      <div className="np-lyric-skeleton" aria-hidden="true">
        <span />
        <span />
        <span />
        <span />
        <span />
      </div>
      <p className="np-lyric-note">
        The music never waits for lyrics. Skeleton lines keep the layout still.
      </p>
    </StateCard>
  );
}

function Footer({
  track,
  leave,
  source,
  providers,
  choose,
}: {
  track: Track;
  leave: () => void;
  source?: { label: string; synced: boolean };
  providers?: Lyrics["state"]["providers"];
  choose?: () => void;
}) {
  return (
    <div className="np-lyric-foot">
      {source && (
        <span>
          {source.synced ? "Synced" : "Not synced"} · from {source.label}
        </span>
      )}
      {providers && providers.length > 1 && choose && (
        <button type="button" className="np-link" onClick={choose}>
          Change
        </button>
      )}
      <RouteLink
        className="np-link"
        href="/settings?section=plugins"
        onNavigate={leave}
      >
        Manage lyrics plugins
      </RouteLink>
      <RouteLink
        className="np-link"
        href={trackTabHref(track.id, "lyrics")}
        onNavigate={leave}
      >
        Track details
      </RouteLink>
    </div>
  );
}

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
    <RouteLink
      className="np-link"
      href={trackTabHref(track.id, "lyrics")}
      onNavigate={leave}
    >
      Track details
    </RouteLink>
  );
  if (state.status === "idle")
    return (
      <StateCard label="Lyrics">
        <p className="np-lyric-note">
          Lyrics are loaded from your lyrics provider when you ask.
        </p>
        <div className="np-lyric-actions">
          <button type="button" className="np-pill" onClick={retry}>
            Load lyrics
          </button>
          {details}
        </div>
      </StateCard>
    );
  if (state.status === "loading") return <Skeleton />;
  if (state.status === "no_provider")
    return (
      <StateCard label="No lyrics plugin" title="No lyrics provider connected">
        <div className="np-lyric-actions">
          <RouteLink
            className="np-pill"
            href="/settings?section=plugins"
            onNavigate={leave}
          >
            Connect one in Settings
          </RouteLink>
        </div>
      </StateCard>
    );
  if (state.status === "failed")
    return (
      <StateCard
        tone="failed"
        label="Couldn't check"
        title="Lyrics could not be loaded."
      >
        <div className="np-lyric-actions">
          <button type="button" className="np-pill" onClick={retry}>
            Try again
          </button>
          <RouteLink
            className="np-link"
            href="/settings?section=plugins"
            onNavigate={leave}
          >
            Try another plugin
          </RouteLink>
        </div>
      </StateCard>
    );
  if (state.status === "none_for_track")
    return (
      <StateCard
        tone="none"
        label="None found"
        title="No lyrics found for this track."
      >
        <div className="np-lyric-actions">
          <button type="button" className="np-pill" onClick={retry}>
            Look up again
          </button>
          {details}
        </div>
      </StateCard>
    );
  const { lyrics, provider, providers } = state;
  if (!lyrics || lyrics.lines.length === 0)
    return (
      <StateCard
        tone="none"
        label="None found"
        title={
          state.unsupported
            ? "This provider has no lyrics for this track."
            : "No lyrics found for this track."
        }
      >
        <div className="np-lyric-actions">
          <button type="button" className="np-pill" onClick={retry}>
            Look up again
          </button>
          {providers.length > 1 && !choosing && (
            <button
              type="button"
              className="np-link"
              onClick={() => setChoosing(true)}
            >
              Choose another provider
            </button>
          )}
          {details}
        </div>
        {choosing && (
          <ProviderChoice
            providers={providers}
            current={provider.observation_id}
            onChoose={(id) => {
              setChoosing(false);
              void choose(id);
            }}
          />
        )}
      </StateCard>
    );
  const synced = isSynced(lyrics.lines);
  return (
    <>
      {synced ? (
        <SyncedLines lines={lyrics.lines} />
      ) : (
        <PlainLines lines={lyrics.lines} />
      )}
      <Footer
        track={track}
        leave={leave}
        source={{ label: provider.label, synced }}
        providers={providers}
        choose={() => setChoosing(!choosing)}
      />
      {choosing && (
        <ProviderChoice
          providers={providers}
          current={provider.observation_id}
          onChoose={(id) => {
            setChoosing(false);
            void choose(id);
          }}
        />
      )}
    </>
  );
}

function ProviderChoice({
  providers,
  current,
  onChoose,
}: {
  providers: Lyrics["state"]["providers"];
  current: string;
  onChoose: (observation: string) => void;
}) {
  return (
    <FilterPills
      label="Lyric provider"
      options={providers.map((item) => ({
        value: item.observation_id,
        label: item.label,
      }))}
      value={current}
      onChange={onChoose}
    />
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
    <section className="np-lyrics" aria-label="Lyrics" data-np-fade="">
      {track ? (
        <TrackLyrics
          key={track.id}
          account={account}
          track={track}
          leave={leave}
        />
      ) : (
        <p className="np-lyric-note">Play a track to see its lyrics.</p>
      )}
    </section>
  );
}
