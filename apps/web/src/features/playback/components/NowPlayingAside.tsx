import type { Schema } from "../../../shared/schema";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { findAudioHref, trackTabHref } from "../../recordings/model/trackTabs";
import {
  bestGuessReason,
  sourceLabels,
  sourceState,
} from "../../sources/model/sourceLabels";
import { usePlayingSource } from "../hooks/usePlayingSource";
import { useQueuePage } from "../hooks/useQueuePage";
import Artwork from "../../artwork/components/Artwork";
import { coverIdentity } from "../../artwork/model/coverIdentity";
import SegmentedControl from "../../../shared/ui/SegmentedControl";
import RouteLink, { visitFromPlayer } from "../../../shared/ui/RouteLink";
import { usePlayer } from "./PlaybackProvider";
import { SPEED_CHOICES } from "./SpeedControl";
import type { PlaybackOccurrence, Track } from "../model";

function availability(track: Track, resolved: Schema<"Source"> | null): string {
  if (resolved?.ready)
    return resolved.provider === "local"
      ? "Ready · Local"
      : `Ready · from ${sourceLabels(resolved).primary}`;
  if (track.available) return "Ready";
  if (track.plays_from_services) return "Ready · from your services";
  return "No source";
}

function UpNextRow({
  account,
  entry,
}: {
  account: string;
  entry: PlaybackOccurrence;
}) {
  const player = usePlayer();
  const resolved = usePlayingSource(account, entry.recording.id).source;
  return (
    <button
      type="button"
      className="np-next-row"
      onClick={() => void player.selectEntry(entry.id)}
    >
      <Artwork
        release={entry.recording.release_id ?? undefined}
        recording={entry.recording.id}
        fallback={coverIdentity(entry.recording)}
      />
      <span>
        <strong>{entry.recording.title}</strong>
        <small>
          {entry.recording.artist} · {availability(entry.recording, resolved)}
        </small>
      </span>
    </button>
  );
}

function UpNextCard({
  account,
  openQueue,
}: {
  account: string;
  openQueue: () => void;
}) {
  const player = usePlayer();
  const { page, error, loading, retry, view } = useQueuePage(player);
  const next = page?.entries[0];
  return (
    <section className="np-card" aria-label="Up next">
      <div className="np-card-head">
        <h2>Up next</h2>
        <RouteLink href="/queue" className="np-link" onNavigate={openQueue}>
          Open queue
        </RouteLink>
      </div>
      {loading && <p className="np-card-note">Loading your queue…</p>}
      {error && (
        <div role="alert" className="np-card-note">
          <p>{error}</p>
          <button type="button" className="np-link" onClick={retry}>
            Retry queue
          </button>
        </div>
      )}
      {!loading && !error && !next && (
        <p className="np-card-note">
          {view.total
            ? "Nothing is queued after this track."
            : "Choose a track to start listening."}
        </p>
      )}
      {next && <UpNextRow account={account} entry={next} />}
    </section>
  );
}

const SPEED_SETTLE_MS = 250;
const label = (speed: number) => `${speed}×`;

export function SpeedCard() {
  const player = usePlayer();
  const speed = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().speed,
  );
  const effective = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().effectiveSpeed ?? player.snapshot().speed,
  );
  const range = useSyncExternalStore(
    player.subscribe,
    () => player.snapshot().speedRange ?? null,
  );
  const ready = useSyncExternalStore(player.subscribe, () => {
    const state = player.snapshot();
    return state.connection === "ready" && state.controlsAvailable !== false;
  });
  const [chosen, setChosen] = useState<number | null>(null);
  const timer = useRef<number | null>(null);
  const latest = useRef({ chosen, player });
  latest.current = { chosen, player };
  useEffect(
    () => () => {
      if (timer.current === null) return;
      window.clearTimeout(timer.current);
      const { chosen: pending, player: owner } = latest.current;
      if (pending !== null) owner.speed(pending);
    },
    [],
  );
  function choose(value: number) {
    setChosen(value);
    latest.current.chosen = value;
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      timer.current = null;
      player.speed(value);
      setChosen(null);
    }, SPEED_SETTLE_MS);
  }
  const shown = chosen ?? speed;
  const choices = SPEED_CHOICES.filter(
    (choice) =>
      choice === shown ||
      range === null ||
      (choice >= range.min && choice <= range.max),
  );
  return (
    <section className="np-card" aria-label="Speed">
      <h2>Speed</h2>
      <SegmentedControl
        label="Speed"
        options={choices.map((choice) => ({
          value: String(choice),
          label: label(choice),
        }))}
        value={String(shown)}
        disabled={!player.supports.speed || !ready}
        onChange={(value) => choose(+value)}
      />
      {chosen === null && effective !== speed && (
        <p className="np-card-note">
          {`This source is playing at ${label(effective)}.`}
        </p>
      )}
    </section>
  );
}

export function SourceCard({
  account,
  track,
  leave,
}: {
  account: string;
  track: Track | null;
  leave: () => void;
}) {
  const playing = usePlayingSource(account, track?.id ?? null);
  const source: Schema<"Source"> | null = playing.source;
  return (
    <section className="np-card" aria-label="This source">
      <h2>Source</h2>
      {!track ? (
        <p className="np-card-note">Nothing is playing.</p>
      ) : playing.loading ? (
        <p className="np-card-note">Checking the source…</p>
      ) : !source ? (
        <p className="np-card-note">This track has no playable source yet.</p>
      ) : (
        <div className="np-source-info">
          <strong>{sourceLabels(source).primary}</strong>
          <span className="np-card-note">{sourceLabels(source).secondary}</span>
          <span className="np-source-chips">
            <span className="track-chip" data-tone={sourceState(source).tone}>
              {sourceState(source).label}
            </span>
            {source.match_kind === "best_guess" && (
              <span
                className="track-chip"
                data-tone="warning"
                title={bestGuessReason(source)}
              >
                Best guess
              </span>
            )}
          </span>
        </div>
      )}
      {track && (
        <div className="np-card-actions">
          <button
            type="button"
            className="np-pill"
            onClick={() => {
              leave();
              visitFromPlayer(findAudioHref(track.id));
            }}
          >
            Change source
          </button>
          <button
            type="button"
            className="np-pill"
            onClick={() => {
              leave();
              visitFromPlayer(trackTabHref(track.id, "sources"));
            }}
          >
            Download
          </button>
        </div>
      )}
    </section>
  );
}

export function SourcePanel({
  account,
  track,
  leave,
}: {
  account: string;
  track: Track | null;
  leave: () => void;
}) {
  const player = usePlayer();
  return (
    <div className="np-panel">
      <SourceCard account={account} track={track} leave={leave} />
      {player.supports.speed && <SpeedCard />}
    </div>
  );
}

export default function NowPlayingAside({
  account,
  track,
  openQueue,
  leave,
}: {
  account: string;
  track: Track | null;
  openQueue: () => void;
  leave: () => void;
}) {
  const player = usePlayer();
  return (
    <aside className="np-aside" aria-label="Up next and source" data-np-fade="">
      <UpNextCard account={account} openQueue={openQueue} />
      <SourceCard account={account} track={track} leave={leave} />
      {player.supports.speed && <SpeedCard />}
    </aside>
  );
}
